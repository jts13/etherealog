use revm::bytecode::{Bytecode, opcode};
use revm::context::{ContextTr, Evm, TxEnv};
use revm::database::EmptyDB;
use revm::handler::instructions::{EthInstructions, InstructionProvider};
use revm::handler::{EthPrecompiles, EvmTr};
use revm::inspector::inspectors::TracerEip3155;
use revm::interpreter::interpreter_types::{Jumps, LoopControl, MemoryTr};
use revm::primitives::{Bytes, TxKind, address};
use revm::state::AccountInfo;
use revm::{Context, InspectEvm, MainBuilder, MainContext};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    // {
    //     "root": "3463104800c5985b196eb96437cdff04e0a669d85a898ff68924353b5f973597",
    //     "accounts": {
    //         "0x00000000000000000000000000000000000000f1": {
    //             "balance": "0",
    //             "nonce": 0,
    //             "root": "0x362d2b556fc3ace7e6b0a2d2ddd306a7bc0cc299f5264d9abd557cde6cd2dbf2",
    //             "codeHash": "0x58c35e4d81bf6b27e1725f0b3c3364b849bace3196710ee574714da41310b492",
    //             "code": "0x604080536040604055604060006040600060ff5afa6040f3",
    //             "storage": {
    //                 "0x0000000000000000000000000000000000000000000000000000000000000040": "40"
    //             },
    //             "address": "0x00000000000000000000000000000000000000f1",
    //             "key": "0xe8c07bab8822eeeb875236e148f781341157f9bfc56c1c53972489ff4009695b"
    //         }
    //     }
    // }

    let mut ctx = Context::mainnet().with_db(EmptyDB::default());

    // let target_address = Address::from_word(b256!(
    //     "0x00000000000000000000000000000000000000000000000000000000000000F0"
    // ));
    // ctx.journal()
    //     .state()
    //     .insert(target_address, Account::default());
    //
    // let caller_address = Address::from_word(b256!(
    //     "0x00000000000000000000000000000000000000000000000000000000000000FF"
    // ));
    // ctx.journal()
    //     .state()
    //     .insert(caller_address, Account::default());

    ctx.journal().state().insert(
        address!("ffffffffffffffffffffffffffffffffffffffff"),
        AccountInfo::from_bytecode(Bytecode::new_raw(Bytes::from(
            &[
                0x60, 0x40, 0x80, 0x53, 0x60, 0x40, 0x60, 0x40, 0x55, 0x60, 0x40, 0x60, 0x00, 0x60,
                0x40, 0x60, 0x00, 0x60, 0xff, 0x5a, 0xfa, 0x60, 0x40, 0xf3,
            ][..],
        )))
        .into(),
    );

    // NOTE(toms): In EVM, it's interesting what happens with a CALL occurs to an address that
    //   either doesn't exist, or doesn't have any code. It's a valid call, and then VM continues
    //   with defined behavior.

    // NOTE(toms): In EVM, it is not possible to return a value directly from the stack. The value
    //   must be first written to memory (e.g. MSTORE), then RETURN'd.

    // state: EVM State is a mapping from addresses to accounts.
    // journal: The journal is a wrapper around the state that tracks changes and allows for e.g. rollbacks.

    ctx.journal().state().insert(
        address!("00000000000000000000000000000000000000ff"),
        AccountInfo::from_bytecode(Bytecode::new_raw(Bytes::from(
            &[opcode::PUSH2, 0xbe, 0xef, opcode::STOP][..],
        )))
        .into(),
    );

    let mut evm = Evm::new_with_inspector(
        ctx,
        TracerEip3155::new_stdout(),
        EthInstructions::new_mainnet(),
        EthPrecompiles::default(),
    );

    // NOTE(toms): gas costs will include 'base stipend' (21000)

    let _ = evm.inspect_with_tx(TxEnv {
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
    });

    Ok(())
}

mod isolate {
    // TODO(toms): tests!
}
