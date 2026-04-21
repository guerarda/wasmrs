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

#[derive(Debug)]
pub struct HostModule {
    pub exports: HashMap<String, ExternVal>,
}

#[derive(Debug)]
pub enum ModuleEntry {
    Wasm(ModuleInstance),
    Host(HostModule),
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub struct ModuleHandle(usize);

#[derive(Debug, Default)]
pub struct ModuleRegistry {
    names: HashMap<String, ModuleHandle>,
    map: HashMap<ModuleHandle, ModuleEntry>,
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
                e.insert(ModuleEntry::Wasm(inst));
            }
            Entry::Occupied(_) => panic!("Handle taken"),
        }
    }

    pub fn register(&mut self, name: String, handle: ModuleHandle) {
        self.names.insert(name, handle);
    }

    pub fn get_instance(&self, handle: ModuleHandle) -> Option<&ModuleInstance> {
        self.map.get(&handle).and_then(|e| {
            if let ModuleEntry::Wasm(m) = e {
                Some(m)
            } else {
                None
            }
        })
    }

    pub fn get_or_create_host(&mut self, name: &str) -> (ModuleHandle, &mut HostModule) {
        if let Some(&mh) = self.names.get(name) {
            if let Some(ModuleEntry::Host(host)) = self.map.get_mut(&mh) {
                return (mh, host);
            }
            panic!("name exist but doesn't map to a module");
        };
        let mh = ModuleHandle(self.next_handle);
        self.next_handle += 1;
        self.names.insert(name.to_string(), mh);

        let ModuleEntry::Host(host) = self.map.entry(mh).or_insert_with(|| {
            ModuleEntry::Host(HostModule {
                exports: HashMap::new(),
            })
        }) else {
            unreachable!();
        };

        (mh, host)
    }

    pub fn resolve(&self, module_name: &str, name: &str) -> Option<ExternVal> {
        let Some(mh) = self.names.get(module_name) else {
            return None;
        };

        let Some(mi) = self.map.get(mh) else {
            panic!("name exist but doesn't map to a module");
        };

        match &mi {
            ModuleEntry::Wasm(inst) => inst.exports.get(name).copied(),
            ModuleEntry::Host(host) => host.exports.get(name).copied(),
        }
    }
}
