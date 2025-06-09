#![allow(clippy::not_unsafe_ptr_arg_deref)]

use common::{parse_plugin_config, AtomImportMap, Config};
use swc_core::{
    common::{util::take::Take, FileName, SyntaxContext, DUMMY_SP},
    ecma::{
        ast::*,
        atoms::Atom,
        utils::{ModuleItemLike, StmtLike},
        visit::{noop_visit_mut_type, Fold, FoldWith, VisitMut, VisitMutWith},
    },
    plugin::{
        metadata::TransformPluginMetadataContextKind, plugin_transform,
        proxies::TransformPluginProgramMetadata,
    },
};

struct DebugLabelTransformVisitor {
    atom_import_map: AtomImportMap,
    current_var_declarator: Option<Id>,
    debug_label_expr: Option<Expr>,
    file_name: FileName,
}

fn create_debug_label_assign_expr(atom_name_id: Id) -> Expr {
    let atom_name = atom_name_id.0.clone();
    Expr::Assign(AssignExpr {
        left: AssignTarget::Simple(SimpleAssignTarget::Member(MemberExpr {
            obj: Box::new(Expr::Ident(atom_name_id.into())),
            prop: MemberProp::Ident("debugLabel".into()),
            span: DUMMY_SP,
        })),
        right: Box::new(Expr::Lit(Lit::Str(Str {
            value: atom_name,
            span: DUMMY_SP,
            raw: None,
        }))),
        op: op!("="),
        span: DUMMY_SP,
    })
}

impl DebugLabelTransformVisitor {
    pub fn new(config: Config, file_name: FileName) -> Self {
        Self {
            atom_import_map: AtomImportMap::new(config.atom_names),
            current_var_declarator: None,
            debug_label_expr: None,
            file_name,
        }
    }
}

impl DebugLabelTransformVisitor {
    fn visit_mut_stmt_like<T>(&mut self, stmts: &mut Vec<T>)
    where
        Vec<T>: VisitMutWith<Self>,
        T: VisitMutWith<Self> + StmtLike + ModuleItemLike,
    {
        let mut stmts_updated: Vec<T> = Vec::with_capacity(stmts.len());

        for stmt in stmts.take() {
            let stmt = match stmt.try_into_stmt() {
                Ok(mut stmt) => {
                    stmt.visit_mut_with(self);
                    T::from(stmt)
                }
                Err(node) => match node.try_into_module_decl() {
                    Ok(mut module_decl) => {
                        match module_decl {
                            ModuleDecl::ExportDefaultExpr(mut default_export) => {
                                if !self.atom_import_map.is_atom_import(&default_export.expr) {
                                    default_export.visit_mut_with(self);
                                    stmts_updated.push(
                                        <T as ModuleItemLike>::try_from_module_decl(
                                            default_export.into(),
                                        )
                                        .unwrap(),
                                    );
                                    continue;
                                }

                                let atom_name: Atom = match &self.file_name {
                                    FileName::Real(real_file_name) => {
                                        if let Some(file_stem) =
                                            real_file_name.file_stem().map(|s| s.to_string_lossy())
                                        {
                                            file_stem.into()
                                        } else {
                                            real_file_name
                                                .parent()
                                                .unwrap()
                                                .join("default_atom")
                                                .display()
                                                .to_string()
                                                .into()
                                        }
                                    }
                                    _ => "default_atom".into(),
                                };

                                // Variable declaration
                                stmts_updated.push(T::from(Stmt::Decl(Decl::Var(Box::new(
                                    VarDecl {
                                        declare: Default::default(),
                                        decls: vec![VarDeclarator {
                                            definite: false,
                                            init: Some(default_export.expr),
                                            name: Pat::Ident(atom_name.clone().into()),
                                            span: DUMMY_SP,
                                        }],
                                        kind: VarDeclKind::Const,
                                        span: DUMMY_SP,
                                        ctxt: SyntaxContext::empty(),
                                    },
                                )))));
                                // Assign debug label
                                stmts_updated.push(T::from(Stmt::Expr(ExprStmt {
                                    span: DUMMY_SP,
                                    expr: Box::new(create_debug_label_assign_expr((
                                        atom_name.clone(),
                                        SyntaxContext::empty(),
                                    ))),
                                })));
                                // export default expression
                                stmts_updated.push(
                                    <T as ModuleItemLike>::try_from_module_decl(
                                        ModuleDecl::ExportDefaultExpr(ExportDefaultExpr {
                                            expr: Box::new(Expr::Ident(atom_name.into())),
                                            span: DUMMY_SP,
                                        }),
                                    )
                                    .unwrap(),
                                );
                                continue;
                            }
                            _ => {
                                module_decl.visit_mut_with(self);
                                <T as ModuleItemLike>::try_from_module_decl(module_decl).unwrap()
                            }
                        }
                    }
                    Err(..) => unreachable!(),
                },
            };
            stmts_updated.push(stmt);

            if self.debug_label_expr.is_none() {
                continue;
            }

            stmts_updated.push(T::from(Stmt::Expr(ExprStmt {
                span: DUMMY_SP,
                expr: Box::new(self.debug_label_expr.take().unwrap()),
            })))
        }

        *stmts = stmts_updated;
    }
}

impl VisitMut for DebugLabelTransformVisitor {
    noop_visit_mut_type!();

    fn visit_mut_import_decl(&mut self, import: &mut ImportDecl) {
        self.atom_import_map.visit_import_decl(import);
    }

    fn visit_mut_var_declarator(&mut self, var_declarator: &mut VarDeclarator) {
        let old_var_declarator = self.current_var_declarator.take();

        self.current_var_declarator = if let Pat::Ident(id) = &var_declarator.name {
            Some(id.to_id())
        } else {
            None
        };

        var_declarator.visit_mut_children_with(self);

        self.current_var_declarator = old_var_declarator;
    }

    fn visit_mut_call_expr(&mut self, call_expr: &mut CallExpr) {
        if self.current_var_declarator.is_none() {
            return;
        }

        call_expr.visit_mut_children_with(self);

        let atom_name = self.current_var_declarator.as_ref().unwrap();
        if let Callee::Expr(expr) = &call_expr.callee {
            if self.atom_import_map.is_atom_import(expr) {
                self.debug_label_expr = Some(create_debug_label_assign_expr(atom_name.clone()))
            }
        }
    }

    fn visit_mut_module_items(&mut self, items: &mut Vec<ModuleItem>) {
        self.visit_mut_stmt_like(items);
    }

    fn visit_mut_stmts(&mut self, stmts: &mut Vec<Stmt>) {
        self.visit_mut_stmt_like(stmts);
    }
}

impl Fold for DebugLabelTransformVisitor {
    fn fold_module(&mut self, mut module: Module) -> Module {
        module.visit_mut_with(self);
        module
    }

    fn fold_script(&mut self, mut script: Script) -> Script {
        script.visit_mut_with(self);
        script
    }
}



pub fn debug_label(config: Config, file_name: FileName) -> impl Fold {
    DebugLabelTransformVisitor::new(config, file_name)
}

#[plugin_transform]
pub fn debug_label_transform(
    program: Program,
    metadata: TransformPluginProgramMetadata,
) -> Program {
    let config = parse_plugin_config(
        &metadata
            .get_transform_plugin_config()
            .expect("Failed to get plugin config for @swc-jotai/debug-label"),
    );
    let file_name = match &metadata.get_context(&TransformPluginMetadataContextKind::Filename) {
        Some(file_name) => FileName::Real(file_name.into()),
        None => FileName::Anon,
    };
    program.fold_with(&mut DebugLabelTransformVisitor::new(
        config, file_name,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swc_core::{
        ecma::{
            parser::Syntax,
            transforms::testing::test_inline,
        },
    };

    fn transform(config: Option<Config>, file_name: Option<FileName>) -> DebugLabelTransformVisitor {
        DebugLabelTransformVisitor::new(
            config.unwrap_or_default(),
            file_name.unwrap_or(FileName::Real("atoms.ts".into()))
        )
    }

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        basic,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        exported_atom,
        r#"
import { atom } from "jotai";
export const countAtom = atom(0);"#,
        r#"
import { atom } from "jotai";
export const countAtom = atom(0);
countAtom.debugLabel = "countAtom";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        multiple_atoms,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
const doubleAtom = atom((get) => get(countAtom) * 2);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";
const doubleAtom = atom((get) => get(countAtom) * 2);
doubleAtom.debugLabel = "doubleAtom";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        multiple_atoms_between_code,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
let counter = 0;
const increment = () => ++counter;
const doubleAtom = atom((get) => get(countAtom) * 2);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";
let counter = 0;
const increment = () => ++counter;
const doubleAtom = atom((get) => get(countAtom) * 2);
doubleAtom.debugLabel = "doubleAtom";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        import_alias,
        r#"
import { atom as blah } from "jotai";
const countAtom = blah(0);"#,
        r#"
import { atom as blah } from "jotai";
const countAtom = blah(0);
countAtom.debugLabel = "countAtom";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        ignore_non_jotai_imports,
        r#"
import React from "react";
import { atom } from "jotai";
import { defaultCount } from "./utils";
const countAtom = atom(0);"#,
        r#"
import React from "react";
import { atom } from "jotai";
import { defaultCount } from "./utils";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        namespace_import,
        r#"
import * as jotai from "jotai";
const countAtom = jotai.atom(0);"#,
        r#"
import * as jotai from "jotai";
const countAtom = jotai.atom(0);
countAtom.debugLabel = "countAtom";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        atom_from_another_package,
        r#"
import { atom } from "some-library";
const countAtom = atom(0);"#,
        r#"
import { atom } from "some-library";
const countAtom = atom(0);"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        no_jotai_import,
        "const countAtom = atom(0);",
        "const countAtom = atom(0);"
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        handle_default_export,
        r#"
import { atom } from "jotai";
export default atom(0);"#,
        r#"
import { atom } from "jotai";
const atoms = atom(0);
atoms.debugLabel = "atoms";
export default atoms;"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, Some(FileName::Real("countAtom.ts".parse().unwrap()))),
        handle_file_naming_default_export,
        r#"
import { atom } from "jotai";
export default atom(0);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";
export default countAtom;"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(
            None,
            Some(FileName::Real("src/atoms/countAtom.ts".parse().unwrap()))
        ),
        handle_file_path_default_export,
        r#"
import { atom } from "jotai";
export default atom(0);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";
export default countAtom;"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        jotai_utils_import,
        r#"
import { atomWithImmer } from "jotai/immer";
import { atomWithMachine } from "jotai/xstate";
const immerAtom = atomWithImmer(0);
const toggleMachineAtom = atomWithMachine(() => toggleMachine);"#,
        r#"
import { atomWithImmer } from "jotai/immer";
import { atomWithMachine } from "jotai/xstate";
const immerAtom = atomWithImmer(0);
immerAtom.debugLabel = "immerAtom";
const toggleMachineAtom = atomWithMachine(() => toggleMachine);
toggleMachineAtom.debugLabel = "toggleMachineAtom";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        test_default_export,
        r#"
function fn() { return true; }

export default fn;"#,
        r#"
function fn() { return true; }

export default fn;"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, None),
        basic_with_existing_debug_label,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "fancyAtomName";"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";
countAtom.debugLabel = "fancyAtomName";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(
            Some(Config {
                atom_names: vec!["customAtom".into()]
            }),
            None
        ),
        custom_atom_names,
        r#"
const myCustomAtom = customAtom(0);"#,
        r#"
const myCustomAtom = customAtom(0);
myCustomAtom.debugLabel = "myCustomAtom";"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, Some(FileName::Anon)),
        filename_anon,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);"#,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
countAtom.debugLabel = "countAtom";"#
    );
}
