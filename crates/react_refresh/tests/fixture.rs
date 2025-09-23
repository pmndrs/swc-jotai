use std::{fs::read_to_string, path::PathBuf};

use common::parse_plugin_config;
use swc_core::{
    common::FileName,
    ecma::parser::{EsSyntax, Syntax},
    ecma::transforms::testing::test_fixture,
};
use swc_jotai_react_refresh::react_refresh;
use testing::fixture;

#[fixture("tests/fixtures/**/input.js")]
fn test(input: PathBuf) {
    let config =
        read_to_string(input.with_file_name("config.json")).expect("Failed to read config.json");
    let config = parse_plugin_config(&config);
    let output = input.with_file_name("output.js");

    test_fixture(
        Syntax::Es(EsSyntax {
            jsx: true,
            ..Default::default()
        }),
        &|_t| react_refresh(config.clone(), FileName::Real("atoms.ts".parse().unwrap())),
        &input,
        &output,
        Default::default(),
    )
}
