//! EVM execution engine with support step-wise execution traces
//!
//! This module provides an [`Engine`] for executing Ethereum Virtual Machine (EVM) code and
//! transactions using the [`revm`] crate. The [`Engine`] provides a trace of the execution of
//! each step of the programs (i.e. smart contracts) in the transaction.
//!
//! # Example
//!
//! ```
//! # use engine::Engine;
//! # use revm::{
//! #     bytecode::Bytecode,
//! #     context::TxEnv,
//! #     primitives::{Bytes, TxKind, address},
//! #     state::AccountInfo,
//! # };
//! fn main() {
//!     let mut engine = Engine::new();
//!
//!     // Create an ephemeral account with a simple bytecode contract/program
//!     let bytecode = Bytecode::new_raw(Bytes::from([0x60, 0x40]));
//!     let account = AccountInfo::from_bytecode(bytecode);
//!     let addr = address!("ffffffffffffffffffffffffffffffffffffffff");
//!     engine.create_account(addr, account);
//!
//!     // Execute a transaction calling the created (contract) account's address
//!     let (res, events) = engine
//!         .execute(TxEnv {
//!             kind: TxKind::Call(addr),
//!             ..Default::default()
//!         })
//!         .unwrap();
//!
//!     // Log the result and state from the executed transaction
//!     println!("res={:?}", res);
//!
//!     // Log the events from the executed transaction
//!     for event in events {
//!         println!("event={event:?}");
//!     }
//! }
//! ```
//!

#![deny(missing_docs)]

use revm::{
    Context, InspectEvm, MainContext,
    context::{
        ContextTr, Evm, JournalTr, TxEnv,
        result::{EVMError, ResultAndState},
    },
    database::EmptyDB,
    handler::{EthPrecompiles, instructions::EthInstructions},
    inspector::{InspectorEvmTr, inspectors::GasInspector},
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs, Interpreter,
        interpreter::EthInterpreter,
        interpreter_types::{Jumps, LoopControl, MemoryTr},
    },
    primitives::{Address, Log, U256, hex},
    state::Account,
};
use serde::Serialize;
use std::convert::Infallible;

/// Ethereum Virtual Machine execution engine with event tracing support
pub struct Engine {
    evm: Evm<Context, Tracer, EthInstructions<EthInterpreter, Context>, EthPrecompiles>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// Constructs a new EVM engine instance with mainnet configuration and tracing enabled
    pub fn new() -> Self {
        Self {
            evm: Evm::new_with_inspector(
                Context::mainnet().with_db(EmptyDB::default()),
                Tracer::new(),
                EthInstructions::new_mainnet(),
                EthPrecompiles::default(),
            ),
        }
    }

    /// Creates a new account in the engine's EVM state
    pub fn create_account(&mut self, address: Address, account: impl Into<Account>) {
        self.evm.journal().state().insert(address, account.into());
    }

    /// Executes a transaction and returns the result and associated events
    pub fn execute(
        &mut self,
        tx: TxEnv,
    ) -> Result<(ResultAndState, Vec<Event>), EVMError<Infallible>> {
        // NOTE(toms): gas costs will include 'base stipend' (21000)
        let res = self.evm.inspect_with_tx(tx)?;
        let events = self.evm.inspector().events.split_off(0);
        Ok((res, events))
    }
}

#[derive(Debug, PartialEq)]
struct StepPre {
    pc: usize,
    op: u8,
    gas: u64,
    stack: Box<[U256]>,
    memory: Option<String>,
}

/// A single step of the EVM engine - inspired by <https://eips.ethereum.org/EIPS/eip-3155>
///
/// # Example (as serialized JSON)
///
/// ```json
/// {
///   "pc": 0,
///   "op": 96,
///   "gas": 2250,
///   "gasCost": 3,
///   "stack": [],
///   "depth": 1
/// }
/// ```
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Program Counter
    pc: usize,
    /// OpCode
    op: u8,
    /// Gas left before executing this operation
    gas: u64,
    /// Gas cost of this operation
    gas_cost: u64,
    /// Array of all values on the stack
    stack: Box<[U256]>,
    /// Depth of the call stack
    depth: u64,
    /// Description of an error (should contain revert reason if supported)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Hex-String representation of all allocated values in memory
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory: Option<String>,
}

/// Tracing events captured during EVM execution
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum Event {
    /// A single step of the EVM engine
    #[serde(rename = "step")]
    Step(Step),
}

struct Tracer {
    gas_inspector: GasInspector,
    step: Option<StepPre>,
    events: Vec<Event>,
}

impl Tracer {
    fn new() -> Self {
        Self {
            gas_inspector: GasInspector::new(),
            step: None,
            events: Default::default(),
        }
    }
}

impl revm::Inspector<Context> for Tracer {
    fn initialize_interp(&mut self, interpreter: &mut Interpreter, _ctx: &mut Context) {
        self.gas_inspector
            .initialize_interp(interpreter.control.gas());
    }

    fn step(&mut self, interpreter: &mut Interpreter, _ctx: &mut Context) {
        self.gas_inspector.step(interpreter.control.gas());

        let pc = interpreter.bytecode.pc();
        let opcode = interpreter.bytecode.opcode();
        let stack = interpreter.stack.data();
        let gas_remaining = interpreter.control.gas().remaining();

        assert_eq!(self.step, None, "Should be empty - consumed by `step_end`");
        self.step = Some(StepPre {
            pc,
            op: opcode,
            stack: stack.clone().into_boxed_slice(),
            gas: gas_remaining,
            memory: if interpreter.memory.size() == 0 {
                None
            } else {
                // TODO(toms): encode as base64 instead? (to save space)
                Some(hex::encode_prefixed(
                    interpreter
                        .memory
                        .slice(0..interpreter.memory.size())
                        .as_ref(),
                ))
            },
        });
    }

    fn step_end(&mut self, interpreter: &mut Interpreter, ctx: &mut Context) {
        self.gas_inspector.step_end(interpreter.control.gas_mut());

        let step = self.step.take().unwrap();

        self.events.push(Event::Step(Step {
            pc: step.pc,
            op: step.op,
            stack: step.stack,
            gas: step.gas,
            gas_cost: self.gas_inspector.last_gas_cost(),
            depth: ctx.journal().depth() as u64,
            error: {
                let result = interpreter.control.instruction_result();
                (result.is_error() || result.is_revert()).then(|| format!("{:?}", result))
            },
            memory: step.memory,
        }));
    }

    fn log(&mut self, _interpreter: &mut Interpreter, _ctx: &mut Context, _log: Log) {}

    fn call(&mut self, _ctx: &mut Context, _inputs: &mut CallInputs) -> Option<CallOutcome> {
        None
    }

    fn call_end(&mut self, _ctx: &mut Context, _inputs: &CallInputs, outcome: &mut CallOutcome) {
        self.gas_inspector.call_end(outcome);
    }

    fn create(&mut self, _ctx: &mut Context, _inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        None
    }

    fn create_end(
        &mut self,
        _ctx: &mut Context,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        self.gas_inspector.create_end(outcome);
    }

    fn eofcreate(
        &mut self,
        _ctx: &mut Context,
        _inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        None
    }

    fn eofcreate_end(
        &mut self,
        _ctx: &mut Context,
        _inputs: &EOFCreateInputs,
        _outcome: &mut CreateOutcome,
    ) {
    }

    fn selfdestruct(&mut self, _contract: Address, _target: Address, _value: U256) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use revm::context::result::HaltReason;
    use revm::primitives::KECCAK_EMPTY;
    use revm::{
        bytecode::{Bytecode, opcode},
        context::{
            TxEnv,
            result::{Output, SuccessReason},
        },
        context_interface::result::ExecutionResult,
        primitives::{Bytes, TxKind, address, hex::FromHex},
        state::AccountInfo,
    };

    macro_rules! assert_matches {
        ($lhs:expr, $rhs:pat $(,)?) => {{ assert!(matches!($lhs, $rhs), "match failed: {:?}", $lhs) }};
    }

    fn stack(values: impl IntoIterator<Item = u64>) -> Box<[U256]> {
        values.into_iter().map(U256::from).collect()
    }

    #[test]
    fn example() {
        let mut engine = Engine::new();

        // # Inspired by <https://eips.ethereum.org/EIPS/eip-3155#test-cases>
        // λ evm run --code '0x604080536040604055604060006040600060ff5afa6040f3'
        //     --json --debug --dump --nomemory=false --noreturndata=false
        //     --sender '0xF0' --receiver '0xF1' --gas 10000000000

        let bytecode = Bytecode::new_raw(
            Bytes::from_hex("604080536040604055604060006040600060ff5afa6040f3").unwrap(),
        );

        engine.create_account(
            address!("ffffffffffffffffffffffffffffffffffffffff"),
            AccountInfo::from_bytecode(bytecode.clone()),
        );

        let (res, events) = engine
            .execute(TxEnv {
                kind: TxKind::Call(address!("ffffffffffffffffffffffffffffffffffffffff")),
                gas_limit: 0x1000000,
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            res.result,
            ExecutionResult::Success {
                reason: SuccessReason::Return,
                gas_used: 0x60a8 + 21000, // includes base stipend
                gas_refunded: 0,
                logs: vec![],
                output: Output::Call([0x40].into()),
            }
        );

        {
            let mut accounts: Vec<_> = res.state.keys().copied().collect();
            accounts.sort();
            assert_eq!(
                accounts,
                [
                    address!("0000000000000000000000000000000000000000"),
                    address!("00000000000000000000000000000000000000ff"),
                    address!("ffffffffffffffffffffffffffffffffffffffff"),
                ]
            );
        }

        let memory = "0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000";

        let expected = &[
            Event::Step(Step {
                pc: 0,
                op: opcode::PUSH1, // 96
                gas: 16756216,
                gas_cost: 3,
                stack: stack([]),
                depth: 1,
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 2,
                op: opcode::DUP1, // 128
                gas: 16756213,
                gas_cost: 3,
                stack: stack([64]),
                depth: 1,
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 3,
                op: opcode::MSTORE8, // 83
                gas: 16756210,
                gas_cost: 12,
                stack: stack([64, 64]),
                depth: 1,
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 4,
                op: opcode::PUSH1, // 96
                gas: 16756198,
                gas_cost: 3,
                stack: stack([]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 6,
                op: opcode::PUSH1, // 96
                gas: 16756195,
                gas_cost: 3,
                stack: stack([64]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 8,
                op: opcode::SSTORE, // 85
                gas: 16756192,
                gas_cost: 22100,
                stack: stack([64, 64]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 9,
                op: opcode::PUSH1, // 96
                gas: 16734092,
                gas_cost: 3,
                stack: stack([]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 11,
                op: opcode::PUSH1, // 96
                gas: 16734089,
                gas_cost: 3,
                stack: stack([64]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 13,
                op: opcode::PUSH1, // 96
                gas: 16734086,
                gas_cost: 3,
                stack: stack([64, 0]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 15,
                op: opcode::PUSH1, // 96
                gas: 16734083,
                gas_cost: 3,
                stack: stack([64, 0, 64]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 17,
                op: opcode::PUSH1, // 96
                gas: 16734080,
                gas_cost: 3,
                stack: stack([64, 0, 64, 0]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 19,
                op: opcode::GAS, // 90
                gas: 16734077,
                gas_cost: 2,
                stack: stack([64, 0, 64, 0, 255]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 20,
                op: opcode::STATICCALL, // 250
                gas: 16734075,
                gas_cost: 16472646,
                stack: stack([64, 0, 64, 0, 255, 16734075]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 21,
                op: opcode::PUSH1, // 96
                gas: 16731475,
                gas_cost: 3,
                stack: stack([1]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
            Event::Step(Step {
                pc: 23,
                op: opcode::RETURN, // 243
                gas: 16731472,
                gas_cost: 0,
                stack: stack([1, 64]),
                depth: 1,
                memory: Some(memory.into()),
                ..Default::default()
            }),
        ];

        let actual = events;
        assert_eq!(actual.len(), expected.len());
        for (n, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
            assert_eq!(actual, expected, "Item {n} did not match!");
        }
    }

    #[test]
    fn empty() {
        let mut engine = Engine::new();

        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        engine.create_account(address, AccountInfo::default());

        let (res, events) = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            res.result,
            ExecutionResult::Success {
                reason: SuccessReason::Stop,
                gas_used: 21000, // base stipend
                gas_refunded: 0,
                logs: vec![],
                output: Output::Call([].into()),
            }
        );

        assert_eq!(events, &[]);
    }

    #[test]
    fn simple() {
        let mut engine = Engine::new();

        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        let bytecode = Bytecode::new_raw(Bytes::from_hex("6040").unwrap());
        engine.create_account(address, AccountInfo::from_bytecode(bytecode));

        let (res, events) = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            res.result,
            ExecutionResult::Success {
                reason: SuccessReason::Stop,
                gas_used: 3 + 21000, // includes base stipend
                gas_refunded: 0,
                logs: vec![],
                output: Output::Call([].into()),
            }
        );

        assert_eq!(
            events,
            &[
                Event::Step(Step {
                    pc: 0,
                    op: opcode::PUSH1, // 96
                    stack: stack([]),
                    gas: 29979000,
                    gas_cost: 3,
                    depth: 1,
                    ..Default::default()
                }),
                Event::Step(Step {
                    pc: 2,
                    op: opcode::STOP, // 0
                    stack: stack([64]),
                    gas: 29978997,
                    gas_cost: 0,
                    depth: 1,
                    ..Default::default()
                })
            ]
        );
    }

    #[test]
    fn selfdestruct() {
        let mut engine = Engine::new();

        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        let bytecode = Bytecode::new_raw(Bytes::from([opcode::PUSH0, opcode::SELFDESTRUCT]));
        engine.create_account(address, AccountInfo::from_bytecode(bytecode));

        let (res, events) = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            res.result,
            ExecutionResult::Success {
                reason: SuccessReason::SelfDestruct,
                gas_used: 26002,
                gas_refunded: 0,
                logs: vec![],
                output: Output::Call([].into()),
            }
        );

        assert_eq!(
            events,
            &[
                Event::Step(Step {
                    pc: 0,
                    op: opcode::PUSH0,
                    stack: stack([]),
                    gas: 29979000,
                    gas_cost: 2,
                    depth: 1,
                    ..Default::default()
                }),
                Event::Step(Step {
                    pc: 1,
                    op: opcode::SELFDESTRUCT,
                    stack: stack([0]),
                    gas: 29978998,
                    gas_cost: 5000,
                    depth: 1,
                    ..Default::default()
                })
            ]
        );
    }

    #[test]
    fn underflow() {
        let mut engine = Engine::new();

        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        let bytecode = Bytecode::new_raw(Bytes::from([opcode::POP]));
        engine.create_account(address, AccountInfo::from_bytecode(bytecode));

        let (res, events) = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            res.result,
            ExecutionResult::Halt {
                reason: HaltReason::StackUnderflow,
                gas_used: 30000000
            }
        );

        assert_eq!(
            events,
            &[Event::Step(Step {
                pc: 0,
                op: opcode::POP,
                stack: stack([]),
                gas: 29979000,
                gas_cost: 2,
                depth: 1,
                error: Some("StackUnderflow".into()),
                ..Default::default()
            })]
        );
    }

    #[test]
    fn keccak256() {
        let mut engine = Engine::new();

        // pseudocode: return keccak256([])
        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        let bytecode = Bytecode::new_raw(Bytes::from([
            opcode::PUSH0, // size
            opcode::PUSH0, // offset
            opcode::KECCAK256,
            opcode::PUSH0, // offset
            opcode::MSTORE,
            opcode::PUSH1, // size
            0x20,          // 32
            opcode::PUSH0, // offset
            opcode::RETURN,
        ]));
        engine.create_account(address, AccountInfo::from_bytecode(bytecode));

        let (res, _events) = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            res.result,
            ExecutionResult::Success {
                reason: SuccessReason::Return,
                gas_used: 21047,
                gas_refunded: 0,
                logs: vec![],
                output: Output::Call(KECCAK_EMPTY.into()),
            }
        );
    }

    #[test]
    fn echo() {
        let mut engine = Engine::new();

        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        let bytecode = Bytecode::new_raw(Bytes::from([
            // ;; copy call-data to memory
            opcode::CALLDATASIZE, // size
            opcode::PUSH0,        // offset
            opcode::PUSH0,        // destOffset
            opcode::CALLDATACOPY,
            // ;; return (copied) call-data
            opcode::CALLDATASIZE, // size
            opcode::PUSH0,        // offset
            opcode::RETURN,
        ]));
        engine.create_account(address, AccountInfo::from_bytecode(bytecode));

        let (res, _events) = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                data: [0xBE, 0xEF, 0x12, 0x34].into(),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            res.result,
            ExecutionResult::Success {
                reason: SuccessReason::Return,
                gas_used: 21160,
                gas_refunded: 0,
                logs: vec![],
                output: Output::Call([0xBE, 0xEF, 0x12, 0x34].into()),
            }
        );

        assert_eq!(res.state.len(), 2);
    }

    #[test]
    fn create2() {
        let mut engine = Engine::new();

        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        let bytecode = Bytecode::new_raw(Bytes::from([
            // ;; copy call-data to memory
            opcode::CALLDATASIZE, // size
            opcode::PUSH0,        // offset
            opcode::PUSH0,        // destOffset
            opcode::CALLDATACOPY,
            // ;; create2
            opcode::PUSH0, // `salt`: 32-byte value used to create the new account at a deterministic address
            opcode::CALLDATASIZE, // `size`: byte size to copy (for initialisation code)
            opcode::PUSH0, // `offset`: byte offset in the memory in bytes (for initialisation code)
            opcode::PUSH0, // `value`: value in wei to send to the new account
            opcode::CREATE2,
        ]));
        engine.create_account(address, AccountInfo::from_bytecode(bytecode));

        let (res, _events) = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                data: [
                    // ;; initialization code (passed as call-data)
                    opcode::PUSH0, // size
                    opcode::PUSH0, // offset
                    opcode::RETURN,
                ]
                .into(),
                ..Default::default()
            })
            .unwrap();

        assert_matches!(
            res.result,
            ExecutionResult::Success {
                reason: SuccessReason::Stop,
                ..
            }
        );

        {
            let mut accounts: Vec<_> = res.state.keys().copied().collect();
            accounts.sort();
            assert_eq!(
                accounts,
                [
                    address!("0000000000000000000000000000000000000000"),
                    address!("2f2d12af7f357a9d2f36d276da056a7c2272decb"),
                    address!("ffffffffffffffffffffffffffffffffffffffff"),
                ]
            );
        }
    }

    #[test]
    fn call() {
        let mut engine = Engine::new();

        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        let bytecode = Bytecode::new_raw(Bytes::from([
            opcode::PUSH0, // `retSize`: byte size to copy (size of the return data).
            opcode::PUSH0, // `retOffset`: byte offset in the memory in bytes, where to store the return data of the sub context.
            opcode::PUSH0, // `argsSize`: byte size to copy (size of the calldata).
            opcode::PUSH0, // `argsOffset`: byte offset in the memory in bytes, the calldata of the sub context.
            opcode::PUSH0, // `value`: value in wei to send to the account.
            opcode::PUSH0, // `address`: the account which context to execute.
            opcode::GAS, // `gas`: amount of gas to send to the sub context to execute. The gas that is not used by the sub context is returned to this one.
            opcode::CALL,
        ]));
        engine.create_account(address, AccountInfo::from_bytecode(bytecode));

        let (res, _events) = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                ..Default::default()
            })
            .unwrap();

        assert_matches!(
            res.result,
            ExecutionResult::Success {
                reason: SuccessReason::Stop,
                ..
            }
        );

        assert_eq!(res.state.len(), 2);
    }
}
