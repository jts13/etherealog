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
use revm::context::{ContextTr, Evm, TxEnv};
use revm::database::EmptyDB;
use revm::handler::EthPrecompiles;
use revm::handler::instructions::EthInstructions;
use revm::inspector::InspectorEvmTr;
use revm::interpreter::interpreter::EthInterpreter;
use revm::interpreter::interpreter_types::{Jumps, LoopControl};
use revm::interpreter::{
    CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs, Interpreter,
};
use revm::primitives::{Address, Log, U256};
use revm::state::Account;
use revm::{Context, InspectEvm, Inspector, MainContext};
use serde::Serialize;
use std::convert::Infallible;

pub struct Engine<I> {
    evm: Evm<Context, I, EthInstructions<EthInterpreter, Context>, EthPrecompiles>,
}

impl<I: Inspector<Context>> Engine<I> {
    pub fn new(inspector: I) -> Self {
        let evm = Evm::new_with_inspector(
            Context::mainnet().with_db(EmptyDB::default()),
            inspector, // Tracer::new(),
            EthInstructions::new_mainnet(),
            EthPrecompiles::default(),
        );

        Self { evm }
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

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum Event {
    Step {
        pc: usize,
        opcode: u8,
        stack: Box<[U256]>,
        gas_remaining: u64,
    },
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
    delegate: D,
}

impl<D> Tracer<D> {
    pub fn new(delegate: D) -> Self {
        Self { delegate }
    }

    pub fn get(&mut self) -> &mut D {
        &mut self.delegate
    }
}

impl<D: TracerDelegate> revm::Inspector<Context> for Tracer<D> {
    fn initialize_interp(&mut self, _interpreter: &mut Interpreter, ctx: &mut Context) {
        // TODO(toms): include initial stipend, etc. (InitialAndFloorGas) in trace log?
        println!(
            ">>> initialize_interp: {:?}",
            (&ctx.tx, &ctx.block, &ctx.cfg)
        );
    }

    fn step(&mut self, interpreter: &mut Interpreter, _ctx: &mut Context) {
        let pc = interpreter.bytecode.pc();
        let opcode = interpreter.bytecode.opcode();
        let stack = interpreter.stack.data();
        let gas_remaining = interpreter.control.gas().remaining();

        // println!(
        //     "pc={pc:?} opcode={opcode:?} stack={stack:?} memSize={} gas_remaining=0x{gas_remaining:x}",
        //     interpreter.memory.size()
        // );

        self.delegate.emit(Event::Step {
            pc,
            opcode,
            stack: stack.clone().into_boxed_slice(),
            gas_remaining,
        });
    }

    fn step_end(&mut self, _interpreter: &mut Interpreter, _ctx: &mut Context) {
        // println!(">>> step_end");
    }

    fn log(&mut self, _interpreter: &mut Interpreter, _ctx: &mut Context, _log: Log) {
        println!(">>> log");
    }

    fn call(&mut self, _ctx: &mut Context, _inputs: &mut CallInputs) -> Option<CallOutcome> {
        println!(">>> call");
        None
    }

    fn call_end(&mut self, _ctx: &mut Context, _inputs: &CallInputs, _outcome: &mut CallOutcome) {
        println!(">>> call_end");
    }

    fn create(&mut self, _ctx: &mut Context, _inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        println!(">>> create");
        None
    }

    fn create_end(
        &mut self,
        _ctx: &mut Context,
        _inputs: &CreateInputs,
        _outcome: &mut CreateOutcome,
    ) {
        println!(">>> create_end");
    }

    fn eofcreate(
        &mut self,
        _ctx: &mut Context,
        _inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        println!(">>> eofcreate");
        None
    }

    fn eofcreate_end(
        &mut self,
        _ctx: &mut Context,
        _inputs: &EOFCreateInputs,
        _outcome: &mut CreateOutcome,
    ) {
        println!(">>> eofcreate_end");
    }

    fn selfdestruct(&mut self, _contract: Address, _target: Address, _value: U256) {
        println!(">>> selfdestruct");
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

        // https://eips.ethereum.org/EIPS/eip-3155#test-cases
        //
        // λ evm run --code '0x604080536040604055604060006040600060ff5afa6040f3'
        //     --json --debug --dump --nomemory=false --noreturndata=false
        //     --sender '0xF0' --receiver '0xF1' --gas 10000000000
        //
        // {"opName":"PUSH1","pc":0,"op":96,"gas":"0x2540be400","gasCost":"0x3","memSize":0,"stack":[],"depth":1,"refund":0}
        // {"opName":"DUP1","pc":2,"op":128,"gas":"0x2540be3fd","gasCost":"0x3","memSize":0,"stack":["0x40"],"depth":1,"refund":0}
        // {"opName":"MSTORE8","pc":3,"op":83,"gas":"0x2540be3fa","gasCost":"0xc","memSize":0,"stack":["0x40","0x40"],"depth":1,"refund":0}
        // {"opName":"PUSH1","pc":4,"op":96,"gas":"0x2540be3ee","gasCost":"0x3","memSize":96,"stack":[],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"PUSH1","pc":6,"op":96,"gas":"0x2540be3eb","gasCost":"0x3","memSize":96,"stack":["0x40"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"SSTORE","pc":8,"op":85,"gas":"0x2540be3e8","gasCost":"0x5654","memSize":96,"stack":["0x40","0x40"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"PUSH1","pc":9,"op":96,"gas":"0x2540b8d94","gasCost":"0x3","memSize":96,"stack":[],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"PUSH1","pc":11,"op":96,"gas":"0x2540b8d91","gasCost":"0x3","memSize":96,"stack":["0x40"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"PUSH1","pc":13,"op":96,"gas":"0x2540b8d8e","gasCost":"0x3","memSize":96,"stack":["0x40","0x0"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"PUSH1","pc":15,"op":96,"gas":"0x2540b8d8b","gasCost":"0x3","memSize":96,"stack":["0x40","0x0","0x40"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"PUSH1","pc":17,"op":96,"gas":"0x2540b8d88","gasCost":"0x3","memSize":96,"stack":["0x40","0x0","0x40","0x0"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"GAS","pc":19,"op":90,"gas":"0x2540b8d85","gasCost":"0x2","memSize":96,"stack":["0x40","0x0","0x40","0x0","0xff"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"STATICCALL","pc":20,"op":250,"gas":"0x2540b8d83","gasCost":"0x24abb5f76","memSize":96,"stack":["0x40","0x0","0x40","0x0","0xff","0x2540b8d83"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"PUSH1","pc":21,"op":96,"gas":"0x2540b835b","gasCost":"0x3","memSize":96,"stack":["0x1"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"opName":"RETURN","pc":23,"op":243,"gas":"0x2540b8358","gasCost":"0x0","memSize":96,"stack":["0x1","0x40"],"depth":1,"refund":0,"memory":"0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000"}
        // {"output":"40","gasUsed":"0x60a8"}

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

        assert_eq!(
            engine.inspector().delegate.events,
            &[
                Event::Step {
                    pc: 0,
                    opcode: opcode::PUSH1, // 96
                    stack: stack([]),
                    gas_remaining: 16756216
                },
                Event::Step {
                    pc: 2,
                    opcode: opcode::DUP1, // 128
                    stack: stack([64]),
                    gas_remaining: 16756213
                },
                Event::Step {
                    pc: 3,
                    opcode: opcode::MSTORE8, // 83
                    stack: stack([64, 64]),
                    gas_remaining: 16756210
                },
                Event::Step {
                    pc: 4,
                    opcode: opcode::PUSH1, // 96
                    stack: stack([]),
                    gas_remaining: 16756198
                },
                Event::Step {
                    pc: 6,
                    opcode: opcode::PUSH1, // 96
                    stack: stack([64]),
                    gas_remaining: 16756195
                },
                Event::Step {
                    pc: 8,
                    opcode: opcode::SSTORE, // 85
                    stack: stack([64, 64]),
                    gas_remaining: 16756192
                },
                Event::Step {
                    pc: 9,
                    opcode: opcode::PUSH1, // 96
                    stack: stack([]),
                    gas_remaining: 16734092
                },
                Event::Step {
                    pc: 11,
                    opcode: opcode::PUSH1, // 96
                    stack: stack([64]),
                    gas_remaining: 16734089
                },
                Event::Step {
                    pc: 13,
                    opcode: opcode::PUSH1, // 96
                    stack: stack([64, 0]),
                    gas_remaining: 16734086
                },
                Event::Step {
                    pc: 15,
                    opcode: opcode::PUSH1, // 96
                    stack: stack([64, 0, 64]),
                    gas_remaining: 16734083
                },
                Event::Step {
                    pc: 17,
                    opcode: opcode::PUSH1, // 96
                    stack: stack([64, 0, 64, 0]),
                    gas_remaining: 16734080
                },
                Event::Step {
                    pc: 19,
                    opcode: opcode::GAS, // 90
                    stack: stack([64, 0, 64, 0, 255]),
                    gas_remaining: 16734077
                },
                Event::Step {
                    pc: 20,
                    opcode: opcode::STATICCALL, // 250
                    stack: stack([64, 0, 64, 0, 255, 16734075]),
                    gas_remaining: 16734075
                },
                Event::Step {
                    pc: 21,
                    opcode: opcode::PUSH1, // 96
                    stack: stack([1]),
                    gas_remaining: 16731475
                },
                Event::Step {
                    pc: 23,
                    opcode: opcode::RETURN, // 243
                    stack: stack([1, 64]),
                    gas_remaining: 16731472
                }
            ]
        );
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
                Event::Step {
                    pc: 0,
                    opcode: 96,
                    stack: stack([]),
                    gas_remaining: 29979000
                },
                Event::Step {
                    pc: 2,
                    opcode: 0,
                    stack: stack([64]),
                    gas_remaining: 29978997
                }
            ]
        );
    }
}
