// https://github.com/ethereum/go-ethereum/tree/master/cmd/evm
/*
// Map of address to account definition.
type Alloc map[common.Address]Account
// Genesis account. Each field is optional.
type Account struct {
    Code       []byte                           `json:"code"`
    Storage    map[common.Hash]common.Hash      `json:"storage"`
    Balance    *big.Int                         `json:"balance"`
    Nonce      uint64                           `json:"nonce"`
    SecretKey  []byte                            `json:"secretKey"`
}

type Env struct {
    // required
    CurrentCoinbase  common.Address      `json:"currentCoinbase"`
    CurrentGasLimit  uint64              `json:"currentGasLimit"`
    CurrentNumber    uint64              `json:"currentNumber"`
    CurrentTimestamp uint64              `json:"currentTimestamp"`
    Withdrawals      []*Withdrawal       `json:"withdrawals"`
    // optional
    CurrentDifficulty *big.Int           `json:"currentDifficulty"`
    CurrentRandom     *big.Int           `json:"currentRandom"`
    CurrentBaseFee    *big.Int           `json:"currentBaseFee"`
    ParentDifficulty  *big.Int           `json:"parentDifficulty"`
    ParentGasUsed     uint64             `json:"parentGasUsed"`
    ParentGasLimit    uint64             `json:"parentGasLimit"`
    ParentTimestamp   uint64             `json:"parentTimestamp"`
    BlockHashes       map[uint64]common.Hash `json:"blockHashes"`
    ParentUncleHash   common.Hash        `json:"parentUncleHash"`
    Ommers            []Ommer            `json:"ommers"`
}

type LegacyTx struct {
    Nonce     uint64          `json:"nonce"`
    GasPrice  *big.Int        `json:"gasPrice"`
    Gas       uint64          `json:"gas"`
    To        *common.Address `json:"to"`
    Value     *big.Int        `json:"value"`
    Data      []byte          `json:"data"`
    V         *big.Int        `json:"v"`
    R         *big.Int        `json:"r"`
    S         *big.Int        `json:"s"`
    SecretKey *common.Hash    `json:"secretKey"`
}
*/

// NOTE(toms): In EVM, it's interesting what happens with a CALL occurs to an address that
//   either doesn't exist, or doesn't have any code. It's a valid call, and then VM continues
//   with defined behavior.

// NOTE(toms): In EVM, it is not possible to return a value directly from the stack. The value
//   must be first written to memory (e.g. MSTORE), then RETURN'd.

// state: EVM State is a mapping from addresses to accounts.
// journal: The journal is a wrapper around the state that tracks changes and allows for e.g. rollbacks.

use revm::context::result::{EVMError, ResultAndState};
use revm::context::{ContextTr, Evm, JournalTr, TxEnv};
use revm::database::EmptyDB;
use revm::handler::EthPrecompiles;
use revm::handler::instructions::EthInstructions;
use revm::inspector::InspectorEvmTr;
use revm::inspector::inspectors::GasInspector;
use revm::interpreter::interpreter::EthInterpreter;
use revm::interpreter::interpreter_types::{Jumps, LoopControl, MemoryTr};
use revm::interpreter::{
    CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs, Interpreter,
};
use revm::primitives::{Address, Log, U256, hex};
use revm::state::Account;
use revm::{Context, InspectEvm, Inspector, MainContext};
use serde::Serialize;
use std::convert::Infallible;

pub struct Engine<I> {
    evm: Evm<Context, I, EthInstructions<EthInterpreter, Context>, EthPrecompiles>,
}

impl<I: Inspector<Context>> Engine<I> {
    pub fn new(inspector: I) -> Self {
        Self {
            evm: Evm::new_with_inspector(
                Context::mainnet().with_db(EmptyDB::default()),
                inspector,
                EthInstructions::new_mainnet(),
                EthPrecompiles::default(),
            ),
        }
    }

    pub fn inspector(&mut self) -> &mut I {
        self.evm.inspector()
    }

    pub fn create_account(&mut self, address: Address, account: impl Into<Account>) {
        self.evm.journal().state().insert(address, account.into());
    }

    pub fn execute(&mut self, tx: TxEnv) -> Result<ResultAndState, EVMError<Infallible>> {
        // NOTE(toms): gas costs will include 'base stipend' (21000)

        self.evm.inspect_with_tx(tx)
    }
}

// TODO(toms): Step (from https://eips.ethereum.org/EIPS/eip-3155)
//   * pc
//   * op
//   * gas
//   * gasCost
//   * memSize
//   * stack
//   * depth
//   * returnData
//   * refund
//   * opName
//   * error
//   * memory
//   * storage

#[derive(Debug, PartialEq)]
struct StepPre {
    pc: usize,
    op: u8,
    gas: u64,
    stack: Box<[U256]>,
    memory: Option<String>,
}

#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Program Counter
    pc: usize,
    /// OpCode
    op: u8,
    /// Gas left before executing this operation
    gas: u64, // U256,
    /// Gas cost of this operation
    gas_cost: u64, // U256,
    /// Array of all values on the stack
    stack: Box<[U256]>,
    /// Depth of the call stack
    depth: u64,
    /// Description of an error (should contain revert reason if supported)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    // TODO(toms): array? string?
    /// Array of all allocated values
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory: Option<String>,
    // /// Data returned by function call
    // return_data: Hex-String,
    // /// Amount of global gas refunded
    // refund: U256,
    // /// Array of all stored values
    // storage: Key-Value,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum Event {
    Step(Step),
}

// TODO(toms): Summary (from https://eips.ethereum.org/EIPS/eip-3155)
//   * stateRoot
//   * output
//   * gasUsed
//   * pass
//   * time
//   * fork

pub trait TracerDelegate {
    fn emit(&mut self, event: Event);
}

pub struct Tracer<D> {
    gas_inspector: GasInspector,
    step: Option<StepPre>,
    delegate: D,
}

impl<D> Tracer<D> {
    pub fn new(delegate: D) -> Self {
        Self {
            gas_inspector: GasInspector::new(),
            step: None,
            delegate,
        }
    }

    pub fn get(&mut self) -> &mut D {
        &mut self.delegate
    }
}

impl<D: TracerDelegate> revm::Inspector<Context> for Tracer<D> {
    fn initialize_interp(&mut self, interpreter: &mut Interpreter, _ctx: &mut Context) {
        self.gas_inspector
            .initialize_interp(interpreter.control.gas());

        // TODO(toms): include initial stipend, etc. (InitialAndFloorGas) in trace log?
        // println!(
        //     ">>> initialize_interp: {:?}",
        //     (&ctx.tx, &ctx.block, &ctx.cfg)
        // );
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

        // self.memory = if self.include_memory {
        //     Some(hex::encode_prefixed(
        //         interp.memory.slice(0..interp.memory.size()).as_ref(),
        //     ))
        // } else {
        //     None
        // };

        // self.refunded = interp.control.gas().refunded();
    }

    fn step_end(&mut self, interpreter: &mut Interpreter, ctx: &mut Context) {
        // println!(">>> step_end");

        self.gas_inspector.step_end(interpreter.control.gas_mut());

        let step = self.step.take().unwrap();

        self.delegate.emit(Event::Step(Step {
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

    fn log(&mut self, _interpreter: &mut Interpreter, _ctx: &mut Context, _log: Log) {
        // println!(">>> log");
    }

    fn call(&mut self, _ctx: &mut Context, _inputs: &mut CallInputs) -> Option<CallOutcome> {
        // println!(">>> call");
        None
    }

    fn call_end(&mut self, _ctx: &mut Context, _inputs: &CallInputs, outcome: &mut CallOutcome) {
        // println!(">>> call_end");
        self.gas_inspector.call_end(outcome);
    }

    fn create(&mut self, _ctx: &mut Context, _inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        // println!(">>> create");
        None
    }

    fn create_end(
        &mut self,
        _ctx: &mut Context,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        // println!(">>> create_end");
        self.gas_inspector.create_end(outcome);
    }

    fn eofcreate(
        &mut self,
        _ctx: &mut Context,
        _inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        // println!(">>> eofcreate");
        None
    }

    fn eofcreate_end(
        &mut self,
        _ctx: &mut Context,
        _inputs: &EOFCreateInputs,
        _outcome: &mut CreateOutcome,
    ) {
        // println!(">>> eofcreate_end");
    }

    fn selfdestruct(&mut self, _contract: Address, _target: Address, _value: U256) {
        // println!(">>> selfdestruct");
    }
}

// TODO(toms): tests!
//   * STATICCALL
//   * CALL
//   * CREATE2
//   * SELFDESTRUCT
//   * KECCAK256
//   * EOF?
//   * Run the code for a _real_ program - ERC-20? key-value store?

// TODO(toms): open questions
//   * What are all the 'inputs' for a smart code execution?
//     * Account storage (key-value storage for smart contract accounts)
//     * Transaction data - CALLDATA
//   * Storage? MLOAD, SLOAD, TLOAD
//   * What are log 'topics'?
//   * BLOBHASH and BLOBBASEFEE - related to BLOBs, introduced as part of EIP-4844 (Proto-Danksharding)
//   * How does SELFDESTRUCT work?
//   * Authorization list? Access list?

#[cfg(test)]
mod tests {
    use super::*;
    use revm::bytecode::{Bytecode, opcode};
    use revm::context::TxEnv;
    use revm::context::result::{Output, SuccessReason};
    use revm::context_interface::result::ExecutionResult;
    use revm::primitives::{Bytes, TxKind, address};
    use revm::state::AccountInfo;

    #[derive(Default)]
    struct TestDelegate {
        events: Vec<Event>,
    }
    impl TracerDelegate for TestDelegate {
        fn emit(&mut self, event: Event) {
            self.events.push(event);
        }
    }

    fn stack(values: impl IntoIterator<Item = u64>) -> Box<[U256]> {
        values.into_iter().map(U256::from).collect()
    }

    // TODO(toms): use external JSONL files as harnesses for tests (for input and output)

    #[test]
    fn experiment() {
        let mut engine = Engine::new(Tracer::new(TestDelegate::default()));

        engine.create_account(
            address!("ffffffffffffffffffffffffffffffffffffffff"),
            AccountInfo::from_bytecode(Bytecode::new_raw(Bytes::from(
                &[
                    0x60, 0x40, 0x80, 0x53, 0x60, 0x40, 0x60, 0x40, 0x55, 0x60, 0x40, 0x60, 0x00,
                    0x60, 0x40, 0x60, 0x00, 0x60, 0xff, 0x5a, 0xfa, 0x60, 0x40, 0xf3,
                ][..],
            ))),
        );

        engine.create_account(
            address!("00000000000000000000000000000000000000ff"),
            AccountInfo::from_bytecode(Bytecode::new_raw(Bytes::from(
                &[opcode::PUSH2, 0xbe, 0xef, opcode::STOP][..],
            ))),
        );

        // TODO(toms): prestate - block environment?

        let _ = engine
            .execute(TxEnv {
                kind: TxKind::Call(address!("ffffffffffffffffffffffffffffffffffffffff")),
                gas_limit: 0x1000000,
                // tx_type: 0,
                // caller: Address::default(),
                // gas_limit: 30_000_000,
                // gas_price: 0,
                // kind: TxKind::Call(Address::default()),
                // value: U256::ZERO,
                // data: Bytes::default(),
                // nonce: 0,
                // chain_id: Some(1), // Mainnet chain ID is 1
                // access_list: Default::default(),
                // gas_priority_fee: Some(0),
                // blob_hashes: Vec::new(),
                // max_fee_per_blob_gas: 0,
                // authorization_list: Vec::new(),
                ..Default::default()
            })
            .unwrap();

        // TODO(toms): assert?!
    }

    #[test]
    fn example() {
        let mut engine = Engine::new(Tracer::new(TestDelegate::default()));

        engine.create_account(
            address!("ffffffffffffffffffffffffffffffffffffffff"),
            AccountInfo::from_bytecode(Bytecode::new_raw(Bytes::from(
                &[
                    0x60, 0x40, 0x80, 0x53, 0x60, 0x40, 0x60, 0x40, 0x55, 0x60, 0x40, 0x60, 0x00,
                    0x60, 0x40, 0x60, 0x00, 0x60, 0xff, 0x5a, 0xfa, 0x60, 0x40, 0xf3,
                ][..],
            ))),
        );

        // TODO(toms): prestate - block environment?

        let result = engine
            .execute(TxEnv {
                kind: TxKind::Call(address!("ffffffffffffffffffffffffffffffffffffffff")),
                gas_limit: 0x1000000,
                ..Default::default()
            })
            .unwrap();

        // # https://eips.ethereum.org/EIPS/eip-3155#test-cases
        // λ evm run --code '0x604080536040604055604060006040600060ff5afa6040f3'
        //     --json --debug --dump --nomemory=false --noreturndata=false
        //     --sender '0xF0' --receiver '0xF1' --gas 10000000000

        // TODO(toms): check result.state?
        assert_eq!(
            result.result,
            ExecutionResult::Success {
                reason: SuccessReason::Return,
                gas_used: 0x60a8 + 21000, // includes base stipend
                gas_refunded: 0,
                logs: vec![],
                output: Output::Call([0x40].into()),
            }
        );

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

        let actual = &engine.inspector().delegate.events;

        assert_eq!(actual.len(), expected.len());
        for (n, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
            assert_eq!(actual, expected, "Item {n} did not match!");
        }
    }

    #[test]
    fn empty() {
        let mut engine = Engine::new(Tracer::new(TestDelegate::default()));

        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        engine.create_account(address, AccountInfo::default());

        let result = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            result.result,
            ExecutionResult::Success {
                reason: SuccessReason::Stop,
                gas_used: 21000, // base stipend
                gas_refunded: 0,
                logs: vec![],
                output: Output::Call([].into()),
            }
        );

        assert_eq!(engine.inspector().delegate.events, &[]);
    }

    #[test]
    fn simple() {
        let mut engine = Engine::new(Tracer::new(TestDelegate::default()));

        let address = address!("ffffffffffffffffffffffffffffffffffffffff");
        engine.create_account(
            address,
            AccountInfo::from_bytecode(Bytecode::new_raw(Bytes::from(&[0x60, 0x40][..]))),
        );

        let result = engine
            .execute(TxEnv {
                kind: TxKind::Call(address),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            result.result,
            ExecutionResult::Success {
                reason: SuccessReason::Stop,
                gas_used: 3 + 21000, // includes base stipend
                gas_refunded: 0,
                logs: vec![],
                output: Output::Call([].into()),
            }
        );

        assert_eq!(
            engine.inspector().delegate.events,
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
}
