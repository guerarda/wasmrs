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
    map: HashMap<ModuleHandle, ModuleInstance>,
    next_handle: usize,
}

impl ModuleRegistry {
    pub fn reserve(&mut self) -> ModuleHandle {
        let h = ModuleHandle(self.next_handle);
        self.next_handle += 1;
        h
    }

    pub fn register(&mut self, handle: ModuleHandle, inst: ModuleInstance) {
        match self.map.entry(handle) {
            Entry::Vacant(e) => {
                e.insert(inst);
            }
            Entry::Occupied(_) => panic!("Handle taken"),
        }
    }

    pub fn get_instance(&self, handle: ModuleHandle) -> &ModuleInstance {
        self.map.get(&handle).unwrap()
    }
}
