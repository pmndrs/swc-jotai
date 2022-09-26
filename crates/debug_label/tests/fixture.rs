use std::path::PathBuf;

use swc_common::chain;
use swc_core::{common::Mark, ecma::transforms::base::resolver};
use swc_ecma_parser::{EsConfig, Syntax};
use swc_ecma_transforms_testing::test_fixture;
use swc_jotai_debug_label::debug_label;
use testing::fixture;

#[fixture("tests/fixtures/**/input.js")]
fn test(input: PathBuf) {
    let output = input.with_file_name("output.js");

    test_fixture(
        Syntax::Es(EsConfig {
            jsx: true,
            ..Default::default()
        }),
        &|_| {
            chain!(
                resolver(Mark::new(), Mark::new(), false),
                debug_label(&PathBuf::from("atoms.ts"))
            )
        },
        &input,
        &output,
    )
}
