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
        &|t| {
            let unresolved_mark = Mark::new();
            let top_level_mark = Mark::new();

            chain!(
                resolver(unresolved_mark, top_level_mark, false),
                debug_label(&PathBuf::from("atoms.ts")),
                swc_ecma_transforms_react::react(
                    t.cm.clone(),
                    Some(t.comments.clone(),),
                    swc_ecma_transforms_react::Options {
                        development: Some(true),
                        refresh: Some(swc_ecma_transforms_react::RefreshOptions {
                            refresh_reg: "$___refreshReg$".into(),
                            refresh_sig: "$___refreshSig$".into(),
                            emit_full_signatures: false
                        }),
                        ..Default::default()
                    },
                    top_level_mark
                ),
                swc_ecma_transforms_compat::es2015(
                    unresolved_mark,
                    Some(t.comments.clone()),
                    Default::default()
                ),
            )
        },
        &input,
        &output,
        Default::default(),
    )
}
