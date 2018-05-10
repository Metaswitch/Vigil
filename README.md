# Vigil

[![Crates.io - Vigil](https://img.shields.io/crates/v/vigil.svg)](https://crates.io/crates/vigil) [![Build Status](https://travis-ci.org/bossmc/vigil.svg?branch=master)](https://travis-ci.org/bossmc/vigil) [![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT) [![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-green.svg)](http://www.apache.org/licenses/LICENSE-2.0)

Software watchdog library for Rust services.

## Documentation

https://docs.rs/vigil/

## Usage

Install from crates.io by adding `vigil` to your `Cargo.toml`:

```
[dependencies]
vigil = "0.1"
```

Now you can create a Vigil instance which the watched code must notify every so often.  If the watched code misses too many notification ticks, the pre-programmed callbacks will be fired to allow you to handle the situation (gather diagnostics, raise an alarm, cancel the stalled task, or even kill the whole process).

```rust
let vigil = Vigil::create(10_000,
                          Some(|| warn!("Watched code missed a watchdog check"),
                          Some(|| error!("Watched code missed multiple watchdog checks!"),
                          Some(|| { error!("Deadlock detected, exiting"); std::process::exit(101));

loop {
  do_work();
  vigil.notify();
}
```
