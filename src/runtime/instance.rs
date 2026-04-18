use std::collections::{HashMap, hash_map::Entry};

use crate::{
    binary::types::FuncType,
    runtime::{
        store::{DataAddr, ElemAddr, FuncAddr, GlobalAddr, MemAddr, TableAddr},
        value::ExternVal,
    },
};

#[derive(Debug, Default)]
pub struct ModuleInstance {
    pub types: Vec<FuncType>,
    pub exports: HashMap<String, ExternVal>,

    pub globals: Vec<GlobalAddr>,
    pub mems: Vec<MemAddr>,
    pub tables: Vec<TableAddr>,
    pub funcs: Vec<FuncAddr>,
    pub datas: Vec<DataAddr>,
    pub elems: Vec<ElemAddr>,
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub struct ModuleHandle(usize);

#[derive(Debug, Default)]
pub struct ModuleRegistry {
    names: HashMap<String, ModuleHandle>,
    map: HashMap<ModuleHandle, ModuleInstance>,
    next_handle: usize,
}

impl ModuleRegistry {
    pub fn reserve(&mut self) -> ModuleHandle {
        let h = ModuleHandle(self.next_handle);
        self.next_handle += 1;
        h
    }

    pub fn add(&mut self, handle: ModuleHandle, inst: ModuleInstance) {
        match self.map.entry(handle) {
            Entry::Vacant(e) => {
                e.insert(inst);
            }
            Entry::Occupied(_) => panic!("Handle taken"),
        }
    }

    pub fn register(&mut self, name: String, handle: ModuleHandle) {
        self.names.insert(name, handle);
    }

    pub fn get_instance(&self, handle: ModuleHandle) -> &ModuleInstance {
        self.map.get(&handle).unwrap()
    }

    pub fn resolve(&self, module_name: &str, name: &str) -> Option<ExternVal> {
        let Some(mh) = self.names.get(module_name) else {
            return None;
        };

        let Some(mi) = self.map.get(mh) else {
            return None;
        };

        mi.exports.get(name).copied()
    }
}
