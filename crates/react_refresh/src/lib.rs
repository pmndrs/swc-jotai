#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use common::{convert_path_to_posix, parse_plugin_config, AtomImportMap, Config};
use swc_core::{
    common::util::take::Take,
    common::DUMMY_SP,
    ecma::{
        ast::*,
        atoms::JsWord,
        utils::{ModuleItemLike, StmtLike, StmtOrModuleItem},
        visit::{as_folder, noop_visit_mut_type, Fold, FoldWith, VisitMut, VisitMutWith},
    },
    plugin::{
        metadata::TransformPluginMetadataContextKind, plugin_transform,
        proxies::TransformPluginProgramMetadata,
    },
    quote,
};

pub struct ReactRefreshTransformVisitor {
    atom_import_map: AtomImportMap,
    current_var_declarator: Option<Id>,
    refresh_atom_var_decl: Option<VarDeclarator>,
    #[allow(dead_code)]
    path: PathBuf,
    exporting: bool,
    top_level: bool,
}

fn create_react_refresh_call_expr(key: String, atom_expr: &CallExpr) -> Box<Expr> {
    Box::new(Expr::Call(CallExpr {
        span: DUMMY_SP,
        callee: Callee::Expr(Box::new(Expr::Member(MemberExpr {
            span: DUMMY_SP,
            obj: Box::new(Expr::Member(MemberExpr {
                span: DUMMY_SP,
                obj: Box::new(Expr::Ident(Ident::new("globalThis".into(), DUMMY_SP))),
                prop: MemberProp::Ident(Ident::new("jotaiAtomCache".into(), DUMMY_SP)),
            })),
            prop: MemberProp::Ident(Ident::new("get".into(), DUMMY_SP)),
        }))),
        args: vec![
            ExprOrSpread {
                spread: None,
                expr: Box::new(Expr::Lit(Lit::Str(Str {
                    value: key.into(),
                    span: DUMMY_SP,
                    raw: None,
                }))),
            },
            ExprOrSpread {
                spread: None,
                expr: Box::new(Expr::Call(atom_expr.clone())),
            },
        ],
        type_args: None,
    }))
}

fn create_react_refresh_var_decl(
    atom_name_id: Id,
    key: String,
    atom_expr: &CallExpr,
) -> VarDeclarator {
    let atom_name = atom_name_id.0.clone();
    VarDeclarator {
        name: Pat::Ident(Ident::new(atom_name, DUMMY_SP.with_ctxt(atom_name_id.1)).into()),
        span: DUMMY_SP.with_ctxt(atom_name_id.1),
        init: Some(create_react_refresh_call_expr(key, atom_expr)),
        definite: false,
    }
}

fn create_cache_key(atom_name: &JsWord, path: &Path) -> String {
    path.display().to_string() + "/" + atom_name
}

impl ReactRefreshTransformVisitor {
    pub fn new(config: Config, path: &Path) -> Self {
        Self {
            atom_import_map: AtomImportMap::new(config.atom_names),
            current_var_declarator: None,
            refresh_atom_var_decl: None,
            path: path.to_owned(),
            exporting: false,
            top_level: false,
        }
    }

    fn visit_mut_stmt_like<T>(&mut self, stmts: &mut Vec<T>)
    where
        Vec<T>: VisitMutWith<Self>,
        T: VisitMutWith<Self> + StmtLike + ModuleItemLike + StmtOrModuleItem,
    {
        let mut stmts_updated: Vec<T> = Vec::with_capacity(stmts.len());
        let mut is_atom_present: bool = false;

        for stmt in stmts.take() {
            let exporting_old = self.exporting;
            let stmt = match stmt.into_stmt() {
                Ok(mut stmt) => {
                    stmt.visit_mut_with(self);
                    <T as StmtLike>::from_stmt(stmt)
                }
                Err(mut module_decl) => match module_decl {
                    ModuleDecl::ExportDefaultExpr(mut default_export) => {
                        if !self.atom_import_map.is_atom_import(&default_export.expr) {
                            default_export.visit_mut_with(self);
                            stmts_updated.push(
                                <T as ModuleItemLike>::try_from_module_decl(default_export.into())
                                    .unwrap(),
                            );
                            continue;
                        }
                        is_atom_present = true;

                        let atom_name: JsWord = self
                            .path
                            .file_stem()
                            .unwrap_or_else(|| OsStr::new("default_atom"))
                            .to_string_lossy()
                            .into();

                        // export default expression
                        stmts_updated.push(
                            <T as ModuleItemLike>::try_from_module_decl(
                                ModuleDecl::ExportDefaultExpr(ExportDefaultExpr {
                                    expr: create_react_refresh_call_expr(
                                        create_cache_key(&atom_name, &self.path),
                                        default_export.expr.as_call().unwrap(),
                                    ),
                                    span: DUMMY_SP,
                                }),
                            )
                            .unwrap(),
                        );
                        continue;
                    }
                    ModuleDecl::ExportDecl(mut export_decl) => {
                        if let Decl::Var(mut var_decl) = export_decl.decl.clone() {
                            if let [VarDeclarator {
                                init: Some(init_expr),
                                ..
                            }] = var_decl.decls.as_mut_slice()
                            {
                                if self.atom_import_map.is_atom_import(&*init_expr) {
                                    self.exporting = true;
                                }
                            }
                        }
                        export_decl.visit_mut_with(self);
                        <T as ModuleItemLike>::try_from_module_decl(export_decl.into()).unwrap()
                    }
                    _ => {
                        module_decl.visit_mut_with(self);
                        <T as ModuleItemLike>::try_from_module_decl(module_decl).unwrap()
                    }
                },
            };

            if self.refresh_atom_var_decl.is_none() {
                stmts_updated.push(stmt);
                continue;
            }

            is_atom_present = true;

            let updated_decl = Decl::Var(Box::new(VarDecl {
                span: DUMMY_SP,
                kind: VarDeclKind::Const,
                declare: false,
                decls: vec![self.refresh_atom_var_decl.take().unwrap()],
            }));

            if self.exporting {
                stmts_updated.push(
                    <T as StmtOrModuleItem>::try_from_module_decl(ModuleDecl::ExportDecl(
                        ExportDecl {
                            span: DUMMY_SP,
                            decl: updated_decl,
                        },
                    ))
                    .unwrap(),
                )
            } else {
                stmts_updated.push(<T as StmtOrModuleItem>::from_stmt(Stmt::Decl(updated_decl)))
            }
            self.exporting = exporting_old;
        }

        if is_atom_present {
            let jotai_cache_stmt = quote!(
                "globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
                cache: new Map(),
                get(name, inst) { 
                  if (this.cache.has(name)) {
                    return this.cache.get(name)
                  }
                  this.cache.set(name, inst)
                  return inst
                },
              }" as Stmt
            );
            let mut stmts_with_cache: Vec<T> = Vec::with_capacity(stmts_updated.len() + 1);
            stmts_with_cache.push(<T as StmtLike>::from_stmt(jotai_cache_stmt));
            stmts_with_cache.append(&mut stmts_updated);
            stmts_updated = stmts_with_cache
        }

        *stmts = stmts_updated;
    }
}

impl VisitMut for ReactRefreshTransformVisitor {
    noop_visit_mut_type!();

    fn visit_mut_import_decl(&mut self, import: &mut ImportDecl) {
        self.atom_import_map.visit_import_decl(import);
    }

    fn visit_mut_var_declarator(&mut self, var_declarator: &mut VarDeclarator) {
        if !self.top_level {
            return;
        }

        let old_var_declarator = self.current_var_declarator.take();

        self.current_var_declarator = if let Pat::Ident(BindingIdent {
            id: Ident { span, sym, .. },
            ..
        }) = &var_declarator.name
        {
            Some((sym.clone(), span.ctxt))
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
                self.refresh_atom_var_decl = Some(create_react_refresh_var_decl(
                    atom_name.clone(),
                    create_cache_key(&atom_name.0, &self.path),
                    call_expr,
                ))
            }
        }
    }

    fn visit_mut_module_items(&mut self, items: &mut Vec<ModuleItem>) {
        self.top_level = true;
        self.visit_mut_stmt_like(items);
    }

    fn visit_mut_stmts(&mut self, stmts: &mut Vec<Stmt>) {
        self.top_level = false;
        self.visit_mut_stmt_like(stmts);
        self.top_level = true;
    }
}

pub fn react_refresh(config: Config, path: &Path) -> impl Fold {
    as_folder(ReactRefreshTransformVisitor::new(config, path))
}

#[plugin_transform]
pub fn react_refresh_transform(
    program: Program,
    metadata: TransformPluginProgramMetadata,
) -> Program {
    let config = parse_plugin_config(
        &metadata
            .get_transform_plugin_config()
            .expect("Failed to get plugin config for @swc-jotai/debug-label"),
    );
    let file_name = convert_path_to_posix(
        &metadata
            .get_context(&TransformPluginMetadataContextKind::Filename)
            .unwrap_or_default(),
    );
    let path = Path::new(&file_name);
    program.fold_with(&mut as_folder(ReactRefreshTransformVisitor::new(
        config, path,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swc_core::{
        common::{chain, Mark},
        ecma::{
            parser::Syntax,
            transforms::{base::resolver, testing::test},
            visit::{as_folder, Fold},
        },
    };

    fn transform(config: Option<Config>, path: Option<&Path>) -> impl Fold {
        chain!(
            resolver(Mark::new(), Mark::new(), false),
            as_folder(ReactRefreshTransformVisitor::new(
                config.unwrap_or_default(),
                path.unwrap_or(&PathBuf::from("atoms.ts"))
            ))
        )
    }

    test!(
        Syntax::default(),
        |_| transform(None, None),
        basic,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atom } from "jotai";
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, None),
        multiple_atoms,
        r#"
import { atom } from "jotai";
const countAtom = atom(0);
const doubleAtom = atom((get) => get(countAtom) * 2);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atom } from "jotai";
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));
const doubleAtom = globalThis.jotaiAtomCache.get("atoms.ts/doubleAtom", atom((get)=>get(countAtom) * 2));"#
    );

    test!(
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
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atom } from "jotai";
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));
let counter = 0;
const increment = () => ++counter;
const doubleAtom = globalThis.jotaiAtomCache.get("atoms.ts/doubleAtom", atom((get)=>get(countAtom) * 2));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, None),
        import_alias,
        r#"
import { atom as blah } from "jotai";
const countAtom = blah(0);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atom as blah } from "jotai";
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", blah(0));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, None),
        ignore_non_jotai_imports,
        r#"
import React from "react";
import { atom } from "jotai";
import { defaultCount } from "./utils";
const countAtom = atom(0);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import React from "react";
import { atom } from "jotai";
import { defaultCount } from "./utils";      
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, None),
        namespace_import,
        r#"
import * as jotai from "jotai";
const countAtom = jotai.atom(0);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import * as jotai from "jotai";
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", jotai.atom(0));"#
    );

    test!(
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

    test!(
        Syntax::default(),
        |_| transform(None, None),
        no_jotai_import,
        "const countAtom = atom(0);",
        "const countAtom = atom(0);"
    );

    test!(
        Syntax::default(),
        |_| transform(None, None),
        handle_default_export,
        r#"
import { atom } from "jotai";
export default atom(0);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atom } from "jotai";
export default globalThis.jotaiAtomCache.get("atoms.ts/atoms", atom(0));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, Some(Path::new("countAtom.ts"))),
        handle_file_naming_default_export,
        r#"
import { atom } from "jotai";
export default atom(0);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atom } from "jotai";
export default globalThis.jotaiAtomCache.get("countAtom.ts/countAtom", atom(0));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, Some(Path::new("src/atoms/countAtom.ts"))),
        handle_file_path_default_export,
        r#"
import { atom } from "jotai";
export default atom(0);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atom } from "jotai";
export default globalThis.jotaiAtomCache.get("src/atoms/countAtom.ts/countAtom", atom(0));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, None),
        jotai_utils_import,
        r#"
import { atomWithImmer } from "jotai/immer";
import { atomWithMachine } from "jotai/xstate";
const immerAtom = atomWithImmer(0);
const toggleMachineAtom = atomWithMachine(() => toggleMachine);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atomWithImmer } from "jotai/immer";
import { atomWithMachine } from "jotai/xstate";
const immerAtom = globalThis.jotaiAtomCache.get("atoms.ts/immerAtom", atomWithImmer(0));
const toggleMachineAtom = globalThis.jotaiAtomCache.get("atoms.ts/toggleMachineAtom", atomWithMachine(()=>toggleMachine));"#
    );

    test!(
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

    test!(
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
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
const myCustomAtom = globalThis.jotaiAtomCache.get("atoms.ts/myCustomAtom", customAtom(0));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, None),
        exported_atom,
        r#"
import { atom } from "jotai";
export const countAtom = atom(0);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
    cache: new Map(),
    get(name, inst) { 
      if (this.cache.has(name)) {
        return this.cache.get(name)
      }
      this.cache.set(name, inst)
      return inst
    },
}        
import { atom } from "jotai";
export const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, None),
        multiple_exported_atoms,
        r#"
import { atom } from "jotai";
export const countAtom = atom(0);
export const doubleAtom = atom((get) => get(countAtom) * 2);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}
import { atom } from "jotai";
export const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));
export const doubleAtom = globalThis.jotaiAtomCache.get("atoms.ts/doubleAtom", atom((get)=>get(countAtom) * 2));"#
    );

    test!(
        Syntax::default(),
        |_| transform(None, None),
        ignore_non_top_level_atoms,
        r#"
import { atom } from "jotai";
function createAtom(ov) {
  const valueAtom = atom(ov);
  const observableValueAtom = atom((get) => {
    const value = get(valueAtom);
    return value;
  },
  (_get, set, nextValue) => {
    set(valueAtom, nextValue);
  });
  return observableValueAtom;
}

const value1Atom = createAtom('Hello String!');
const countAtom = atom(0);"#,
        r#"
globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) { 
    if (this.cache.has(name)) {
      return this.cache.get(name)
    }
    this.cache.set(name, inst)
    return inst
  },
}        
import { atom } from "jotai";
function createAtom(ov) {
  const valueAtom = atom(ov);
  const observableValueAtom = atom((get) => {
    const value = get(valueAtom);
    return value;
  },
  (_get, set, nextValue) => {
    set(valueAtom, nextValue);
  });
  return observableValueAtom;
}

const value1Atom = createAtom('Hello String!');
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));"#
    );
}
