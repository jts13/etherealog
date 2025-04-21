use engine::{Engine, Event, Tracer, TracerDelegate};
use revm::context::result::ResultAndState;
use revm::{
    bytecode::Bytecode,
    context::TxEnv,
    primitives::{Address, Bytes, TxKind, U256, address},
    state::{AccountInfo, EvmStorage},
};
use rocket::{
    fs::{FileServer, Options},
    serde::json::Json,
};
use rocket_okapi::{rapidoc::*, settings::UrlObject, swagger_ui::*};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Default)]
struct Delegate {
    events: Vec<Event>,
}

impl TracerDelegate for Delegate {
    fn emit(&mut self, event: Event) {
        self.events.push(event);
    }
}

#[derive(Debug, serde::Serialize)]
struct Response {
    events: Vec<Event>,
    // TODO(toms): refine response object in line with <https://eips.ethereum.org/EIPS/eip-3155>
    summary: ResultAndState,
}

#[rocket::post("/api/isolate/eval/<code>")]
fn eval(code: &str) -> Result<Json<Response>, String> {
    let mut engine = Engine::new(Tracer::new(Delegate::default()));

    let addr = address!("ffffffffffffffffffffffffffffffffffffffff");

    engine.create_account(
        addr,
        AccountInfo::from_bytecode(Bytecode::new_raw(
            Bytes::from_str(code).map_err(|err| err.to_string())?,
        )),
    );

    let res = engine
        .execute(TxEnv {
            kind: TxKind::Call(addr),
            gas_limit: 0x1000000,
            ..Default::default()
        })
        .map_err(|err| err.to_string())?;

    Ok(Json(Response {
        events: engine.inspector().get().events.split_off(0),
        summary: res,
    }))
}

#[derive(Debug, Serialize, Deserialize)]
struct Account {
    address: Address,
    balance: U256,
    nonce: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<Bytes>,
    storage: EvmStorage,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
enum Transaction {
    Call { address: Address },
}

#[derive(Debug, Serialize, Deserialize)]
struct Environment {
    accounts: Box<[Account]>,
    transaction: Transaction,
}

#[rocket::post("/api/isolate/transaction", data = "<environment>")]
fn transaction(environment: Json<Environment>) -> Result<Json<Response>, String> {
    let mut engine = Engine::new(Tracer::new(Delegate::default()));

    let environment = environment.0;

    for Account {
        address,
        balance,
        nonce,
        code,
        storage,
    } in environment.accounts
    {
        engine.create_account(
            address,
            revm::state::Account::from(match code {
                None => AccountInfo::from_balance(balance).with_nonce(nonce),
                Some(code) => AccountInfo::from_bytecode(Bytecode::new_raw(code)),
            })
            .with_storage(storage.into_iter()),
        );
    }

    let res = engine
        .execute(TxEnv {
            kind: match environment.transaction {
                Transaction::Call { address } => TxKind::Call(address),
            },
            gas_limit: 0x1000000,
            ..Default::default()
        })
        .map_err(|err| err.to_string())?;

    Ok(Json(Response {
        events: engine.inspector().get().events.split_off(0),
        summary: res,
    }))
}

#[rocket::launch]
fn rocket() -> _ {
    rocket::build()
        .mount("/", rocket::routes![eval, transaction])
        .mount("/res", FileServer::new("res", Options::default()))
        .mount(
            "/swagger-ui/",
            make_swagger_ui(&SwaggerUIConfig {
                url: "/res/openapi.json".to_owned(),
                ..Default::default()
            }),
        )
        .mount(
            "/rapidoc/",
            make_rapidoc(&RapiDocConfig {
                general: GeneralConfig {
                    spec_urls: vec![UrlObject::new("General", "/res/openapi.json")],
                    ..Default::default()
                },
                hide_show: HideShowConfig {
                    allow_spec_url_load: false,
                    allow_spec_file_load: false,
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
}
