use swc_core::{
    common::collections::AHashSet,
    ecma::{ast::*, atoms::JsWord},
};

use crate::ATOM_IMPORTS;

#[derive(Debug)]
pub struct AtomImportMap {
    atom_names: Vec<JsWord>,
    imports: AHashSet<JsWord>,
    namespace_imports: AHashSet<JsWord>,
}

impl AtomImportMap {
    pub fn new(atom_names: Vec<JsWord>) -> Self {
        AtomImportMap {
            atom_names,
            imports: Default::default(),
            namespace_imports: Default::default(),
        }
    }

    pub fn visit_import_decl(&mut self, import: &ImportDecl) {
        if !import.src.value.starts_with("jotai") {
            return;
        }

        for s in &import.specifiers {
            let local_ident = match s {
                ImportSpecifier::Named(ImportNamedSpecifier {
                    local,
                    imported: Some(ModuleExportName::Ident(ident)),
                    ..
                }) => {
                    if ATOM_IMPORTS.contains(&&*ident.sym) {
                        local.sym.clone()
                    } else {
                        continue;
                    }
                }
                ImportSpecifier::Named(ImportNamedSpecifier { local, .. }) => {
                    if ATOM_IMPORTS.contains(&&*local.sym) {
                        local.sym.clone()
                    } else {
                        continue;
                    }
                }
                ImportSpecifier::Namespace(..) => {
                    self.namespace_imports.insert(import.src.value.clone());
                    continue;
                }
                _ => continue,
            };

            self.imports.insert(local_ident);
        }
    }

    pub fn is_atom_import(&self, expr: &Expr) -> bool {
        match expr {
            // Handles default export expressions
            Expr::Call(CallExpr {
                callee: Callee::Expr(e),
                ..
            }) => self.is_atom_import(e),
            // Handles: const countAtom = atom(0);
            Expr::Ident(i) => {
                self.atom_names.contains(&i.sym) || self.imports.get(&i.sym).is_some()
            }
            // Handles: const countAtom = jotai.atom(0);
            Expr::Member(MemberExpr {
                obj,
                prop: MemberProp::Ident(prop),
                ..
            }) => {
                if let Expr::Ident(obj) = &**obj {
                    if let Some(..) = self.namespace_imports.get(&obj.sym) {
                        return ATOM_IMPORTS.contains(&&*prop.sym);
                    }
                }
                false
            }
            _ => false,
        }
    }
}
