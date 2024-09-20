#![allow(clippy::not_unsafe_ptr_arg_deref)]

use common::{parse_plugin_config, AtomImportMap, Config};
use swc_core::{
    common::{FileName, SyntaxContext, DUMMY_SP},
    ecma::{
        ast::*,
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
    #[allow(dead_code)]
    file_name: FileName,
    /// We're currently at the top level
    top_level: bool,
    /// Any atom was used.
    used_atom: bool,
    /// Path to the current expression when walking object and array literals.
    /// For instance, when walking this expression:
    /// ```js
    /// const foo = [{}, { bar: [ 123 ]}]
    /// ```
    /// the path will be `["foo", "1", "bar", "0"]` when visiting `123`.
    access_path: Vec<String>,
}

fn create_react_refresh_call_expr_(key: String, atom_expr: &CallExpr) -> CallExpr {
    CallExpr {
        span: DUMMY_SP,
        ctxt: SyntaxContext::empty(),
        callee: Callee::Expr(Box::new(Expr::Member(MemberExpr {
            span: DUMMY_SP,
            obj: Box::new(Expr::Member(MemberExpr {
                span: DUMMY_SP,
                obj: Box::new(Expr::Ident("globalThis".into())),
                prop: MemberProp::Ident("jotaiAtomCache".into()),
            })),
            prop: MemberProp::Ident("get".into()),
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
    }
}

fn show_prop_name(pn: &PropName) -> String {
    use PropName::*;
    match pn {
        Ident(ref i) => i.sym.to_string(),
        Str(ref s) => s.value.to_string(),
        Num(ref n) => n
            .raw
            .as_ref()
            .expect("Num(c).raw should be Some")
            .to_string(),
        Computed(ref c) => format!("computed:{:?}", c.span),
        BigInt(ref b) => b
            .raw
            .as_ref()
            .expect("BigInt(b).raw should be Some")
            .to_string(),
    }
}

impl ReactRefreshTransformVisitor {
    pub fn new(config: Config, file_name: FileName) -> Self {
        Self {
            atom_import_map: AtomImportMap::new(config.atom_names),
            file_name,
            top_level: false,
            used_atom: false,
            access_path: Vec::new(),
        }
    }

    fn create_cache_key(&self) -> String {
        match self.file_name {
            FileName::Real(ref real_file_name) => format!(
                "{}/{}",
                real_file_name.display(),
                self.access_path.join(".")
            ),
            _ => self.access_path.join("."),
        }
    }
}

impl VisitMut for ReactRefreshTransformVisitor {
    noop_visit_mut_type!();

    fn visit_mut_import_decl(&mut self, import: &mut ImportDecl) {
        self.atom_import_map.visit_import_decl(import);
    }

    fn visit_mut_module_items(&mut self, items: &mut Vec<ModuleItem>) {
        self.top_level = true;
        items.visit_mut_children_with(self);
        if self.used_atom {
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
            let mi: ModuleItem = jotai_cache_stmt.into();
            items.insert(0, mi);
        }
    }

    fn visit_mut_stmts(&mut self, stmts: &mut Vec<Stmt>) {
        let top_level = self.top_level;
        self.top_level = false;
        stmts.visit_mut_children_with(self);
        self.top_level = top_level;
    }

    fn visit_mut_var_declarator(&mut self, var_declarator: &mut VarDeclarator) {
        if !self.top_level {
            return;
        }

        let key = if let Pat::Ident(BindingIdent {
            id: Ident { sym, .. },
            ..
        }) = &var_declarator.name
        {
            sym.to_string()
        } else {
            "[missing-declarator]".to_string()
        };

        self.access_path.push(key);
        var_declarator.visit_mut_children_with(self);
        self.access_path.pop();
    }

    fn visit_mut_arrow_expr(&mut self, _: &mut ArrowExpr) {
        // Arrow expressions is (maybe) the only way for expressions to not be at the top level and
        // not have us visit `stmts` before.  Since we record whether we're on the top level in
        // `visit_mut_stmts`, we need to make sure we don't visit the body here, so that any atoms
        // aren't erroneously cached.
    }

    fn visit_mut_array_lit(&mut self, array: &mut ArrayLit) {
        if !self.top_level {
            return;
        }
        for (i, child) in array.elems.iter_mut().enumerate() {
            self.access_path.push(i.to_string());
            child.visit_mut_with(self);
            self.access_path.pop();
        }
    }

    fn visit_mut_object_lit(&mut self, object: &mut ObjectLit) {
        if !self.top_level {
            return;
        }
        // For each prop in the object we need to record the path down to build up the ind-path
        // down to any atoms in the literal.
        for prop in object.props.iter_mut() {
            match prop {
                PropOrSpread::Prop(ref mut prop) => match prop.as_mut() {
                    Prop::Shorthand(ref mut s) => {
                        self.access_path.push(s.sym.to_string());
                        prop.visit_mut_with(self);
                        self.access_path.pop();
                    }
                    Prop::KeyValue(ref mut kv) => {
                        self.access_path.push(show_prop_name(&kv.key));
                        prop.visit_mut_with(self);
                        self.access_path.pop();
                    }
                    _ => prop.visit_mut_with(self),
                },
                _ => prop.visit_mut_with(self),
            }
        }
    }

    fn visit_mut_call_expr(&mut self, call_expr: &mut CallExpr) {
        // If this is an atom, replace it with the cached `get` expression.
        if self.top_level {
            if let Callee::Expr(expr) = &call_expr.callee {
                if self.atom_import_map.is_atom_import(expr) {
                    *call_expr =
                        create_react_refresh_call_expr_(self.create_cache_key(), call_expr);
                    self.used_atom = true;
                    return;
                }
            }
        }
        call_expr.visit_mut_children_with(self);
    }
}

pub fn react_refresh(config: Config, file_name: FileName) -> impl Fold {
    as_folder(ReactRefreshTransformVisitor::new(config, file_name))
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
    let file_name = match &metadata.get_context(&TransformPluginMetadataContextKind::Filename) {
        Some(file_name) => FileName::Real(file_name.into()),
        None => FileName::Anon,
    };
    program.fold_with(&mut as_folder(ReactRefreshTransformVisitor::new(
        config, file_name,
    )))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use swc_core::{
        common::{chain, Mark},
        ecma::{
            parser::Syntax,
            transforms::{
                base::resolver,
                testing::{test, test_inline},
            },
            visit::{as_folder, Fold},
        },
    };

    fn transform(config: Option<Config>, file_name: Option<FileName>) -> impl Fold {
        chain!(
            resolver(Mark::new(), Mark::new(), false),
            as_folder(ReactRefreshTransformVisitor::new(
                config.unwrap_or_default(),
                file_name.unwrap_or(FileName::Real(PathBuf::from("atoms.ts")))
            ))
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

    test_inline!(
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

    test_inline!(
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

    test_inline!(
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
export default globalThis.jotaiAtomCache.get("atoms.ts/", atom(0));"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, Some(FileName::Real("countAtom.ts".parse().unwrap()))),
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
export default globalThis.jotaiAtomCache.get("countAtom.ts/", atom(0));"#
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
export default globalThis.jotaiAtomCache.get("src/atoms/countAtom.ts/", atom(0));"#
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

    test_inline!(
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

    test_inline!(
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

    test_inline!(
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

    test_inline!(
        Syntax::default(),
        |_| transform(None, Some(FileName::Anon)),
        nested_top_level_atoms,
        r#"
import { atom } from "jotai";

const three = atom(atom(atom(0)));
"#,
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
const three = globalThis.jotaiAtomCache.get("three", atom(atom(atom(0))));
"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, Some(FileName::Anon)),
        higher_order_fn_to_atom,
        r#"
import { atom } from "jotai";

function getAtom() {
    return atom(1);
}
const getAtom2 = () => atom(2);
const getAtom3 = () => { return atom(3) };
"#,
        r#"
import { atom } from "jotai";

function getAtom() {
    return atom(1);
}
const getAtom2 = () => atom(2);
const getAtom3 = () => { return atom(3) };
"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, Some(FileName::Anon)),
        atom_in_atom_reader_stmt,
        r#"
import { atom } from "jotai";

export const state = atom(() => {
   return atom(0);
});"#,
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

export const state = globalThis.jotaiAtomCache.get("state", atom(() => {
    return atom(0);
}));"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, Some(FileName::Anon)),
        array_and_object_top_level,
        r#"
import { atom } from "jotai";

const arr = [
    atom(3),
    atom(4),
];

const obj = {
    five: atom(5),
    six: atom(6),
};

function keepThese() {
    const a = [atom(7)];
    const b = { eight: atom(8) };
}
"#,
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

const arr = [
    globalThis.jotaiAtomCache.get("arr.0", atom(3)),
    globalThis.jotaiAtomCache.get("arr.1", atom(4)),
];

const obj = {
    five: globalThis.jotaiAtomCache.get("obj.five", atom(5)),
    six: globalThis.jotaiAtomCache.get("obj.six", atom(6)),
};

function keepThese() {
    const a = [atom(7)];
    const b = { eight: atom(8) };
}
"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, Some(FileName::Anon)),
        object_edge_cases,
        r#"
import { atom } from "jotai";

const obj = {
    five: atom(5),
    six: atom(6),
    ...({
        six: atom(66),
    })
};
"#,
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

const obj = {
    five: globalThis.jotaiAtomCache.get("obj.five", atom(5)),
    six: globalThis.jotaiAtomCache.get("obj.six", atom(6)),
    ...{
        six: globalThis.jotaiAtomCache.get("obj.six", atom(66)),
    }
};
"#
    );

    test_inline!(
        Syntax::default(),
        |_| transform(None, Some(FileName::Anon)),
        compound_export,
        r#"
import { atom } from "jotai";

export const one = atom(1),
             two = atom(2);
"#,
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

export const one = globalThis.jotaiAtomCache.get("one", atom(1)), two = globalThis.jotaiAtomCache.get("two", atom(2));
"#
    );
}
