# `etherealog`

## Summary

`etherealog` is a Rust-based Ethereum Virtual Machine (EVM) execution engine that provides step-wise execution
tracing. The execution engine can be leverage via the provided REST API services.

## Getting Started

To launch the REST APIs and the associated Swagger page, run the following and navigate
to http://127.0.0.1:8000/swagger-ui/:

```shell
cargo run --release -p services
```

To execute the unit tests for the project (in particular, the core engine), run the following:

```shell
cargo test
```

If you're new to Rust, [`rustup`](https://rustup.rs/) is the standard installer for Rust.

## Features

* **Step-wise Tracing** — Captures each EVM opcode step, including stack, memory, gas usage, and errors.
* **Isolated Execution Environments** — Supports evaluating EVM bytecode in a self-contained and self-defined context.
* **REST API Endpoints** — Offers REST APIs to evaluate bytecode or simulate transactions via [
  `rocket`](https://rocket.rs/) :rocket:.

## API Overview

* `POST /api/isolate/eval/<code>`
    * Evaluate raw EVM bytecode and return the trace events and result.

* `POST /api/isolate/transaction`
    * Simulate an isolated EVM transaction using the specified accounts and initial state.

## Team Structure & Work Breakdown

* Tom Schroeder
    * Core Engine
    * REST API service
    * OpenAPI definition
    * Unit test suite
* Taobo Liao
* Bach Hoang

## Future Improvements

* Solidity compilation support
* Contract debugger/replay
* EOF (EVM Object Format) support
