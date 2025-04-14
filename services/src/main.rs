use engine::{Engine, Event, Tracer, TracerDelegate};
use revm::{
    bytecode::Bytecode,
    context::TxEnv,
    primitives::{Bytes, TxKind, address},
    state::AccountInfo,
};
use rocket::{
    fs::{FileServer, Options},
    serde::json::Json,
};
use rocket_okapi::{rapidoc::*, settings::UrlObject, swagger_ui::*};
use std::str::FromStr;

// TODO(toms): 'test' endpoints
//   * POST /api/health-check
// TODO(toms): 'isolate' endpoints
//   * POST /api/isolate/transaction - execute a transaction in a given state/environment
//     * prestate - block environment?

// https://learn.openapis.org/examples/v3.0/petstore.html

#[derive(Default)]
struct Delegate {
    events: Vec<Event>,
}

impl TracerDelegate for Delegate {
    fn emit(&mut self, event: Event) {
        self.events.push(event);
    }
}

#[derive(serde::Serialize)]
struct Response {
    events: Vec<Event>,
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

    let _ = engine
        .execute(TxEnv {
            kind: TxKind::Call(addr),
            gas_limit: 0x1000000,
            ..Default::default()
        })
        .map_err(|err| err.to_string())?;

    Ok(Json(Response {
        events: engine.inspector().get().events.split_off(0),
    }))
}

#[derive(Debug, serde::Deserialize)]
struct Transaction {
    foo: String,
    bar: String,
}

#[rocket::post("/api/isolate/transaction", data = "<transaction>")]
fn transaction(transaction: Json<Transaction>) -> Result<Json<Response>, String> {
    let mut engine = Engine::new(Tracer::new(Delegate::default()));

    let transaction = transaction.0;
    println!("transaction={transaction:?}");

    let addr = address!("ffffffffffffffffffffffffffffffffffffffff");

    engine.create_account(
        addr,
        AccountInfo::from_bytecode(Bytecode::new_raw(
            Bytes::from_str("6040").map_err(|err| err.to_string())?,
        )),
    );

    let _ = engine
        .execute(TxEnv {
            kind: TxKind::Call(addr),
            gas_limit: 0x1000000,
            ..Default::default()
        })
        .map_err(|err| err.to_string())?;

    Ok(Json(Response {
        events: engine.inspector().get().events.split_off(0),
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
