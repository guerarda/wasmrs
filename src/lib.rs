use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io::{Seek, SeekFrom};
use std::iter::repeat_n;

mod leb128;
mod reader;

use crate::instructions::Instruction;
use crate::reader::ReadErrorKind;
use crate::sections::{
    CodeSection, ExportSection, FunctionSection, SectionError, SectionId, TypeSection,
    read_code_section, read_export_section, read_function_section, read_type_section,
};
use crate::types::{FuncType, TypeIdx, ValType};
use reader::{ReadError, Reader};

mod sections;
use crate::sections::SectionInfo;

mod instructions;
mod types;

const WASM_MAGIC: [u8; 4] = *b"\0asm";
const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

#[derive(Debug)]
struct Module {
    bytes: Vec<u8>,
    sections: Vec<SectionInfo>,

    types: Option<TypeSection>,
    functions: Option<FunctionSection>,
    exports: Option<ExportSection>,
    codes: Option<CodeSection>,
}

impl Module {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        Module {
            bytes,
            sections: Vec::new(),

            types: None,
            functions: None,
            exports: None,
            codes: None,
        }
    }
}

struct ModuleReader<'a> {
    reader: Reader<'a>,
}

impl<'a> ModuleReader<'a> {
    fn from_module(module: &'a Module) -> Self {
        ModuleReader {
            reader: Reader::from_bytes(&module.bytes, 0),
        }
    }

    fn read_preamble(&mut self) -> reader::Result<()> {
        let mut buf = [0u8; 4];

        self.reader.read_exact(&mut buf)?;
        if buf != WASM_MAGIC {
            return Err(ReadError::at_offset(ReadErrorKind::BadMagic, 0));
        }

        self.reader.read_exact(&mut buf)?;
        if buf != WASM_VERSION {
            return Err(ReadError::at_offset(ReadErrorKind::BadVersion, 4));
        }
        Ok(())
    }

    fn read_toc(&mut self) -> reader::Result<Vec<SectionInfo>> {
        let mut v = Vec::new();
        while self.reader.has_data_left()? {
            let offset = self.reader.position() as usize;
            let id: SectionId = self.reader.read_u8()?.try_into().map_err(|e| ReadError {
                kind: ReadErrorKind::InvalidEnumValue(e),
                offset,
            })?;
            let size = self.reader.read_u32()?;

            let info = SectionInfo {
                id,
                start: self.reader.position(),
                end: self
                    .reader
                    .cursor
                    .seek(SeekFrom::Current(size as i64))
                    .unwrap(),
                size,
            };
            v.push(info);
        }
        Ok(v)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FuncAddr(usize);

impl From<usize> for FuncAddr {
    fn from(value: usize) -> Self {
        FuncAddr(value)
    }
}

impl TryFrom<&ExternVal> for FuncAddr {
    type Error = anyhow::Error;

    fn try_from(value: &ExternVal) -> std::result::Result<Self, Self::Error> {
        match *value {
            ExternVal::Func(funcaddr) => Ok(funcaddr),
            _ => panic!("oops"),
        }
    }
}

#[derive(Debug)]
pub enum ExternVal {
    Func(FuncAddr),
    Table(usize),
    Mem(usize),
    Global(usize),
}

#[derive(Debug, Default)]
pub struct ModuleInstance {
    types: Vec<FuncType>,
    funcaddrs: Vec<FuncAddr>,
    exports: HashMap<String, ExternVal>,
}

#[derive(Debug)]
pub struct Func {
    #[allow(dead_code)]
    typeidx: TypeIdx,
    locals: Vec<ValType>,
    body: Vec<Instruction>,
}

#[derive(Debug)]
pub struct FuncInstance {
    ftype: FuncType,
    module: ModuleHandle,
    func: Func,
}

#[derive(Debug, Default)]
pub struct Store {
    funcs: Vec<FuncInstance>,
}

impl Store {
    pub fn get_func(&self, addr: FuncAddr) -> &FuncInstance {
        &self.funcs[addr.0]
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub struct ModuleHandle(usize);

#[derive(Debug, Default)]
struct ModuleRegistry {
    map: HashMap<ModuleHandle, ModuleInstance>,
    next_handle: usize,
}

impl ModuleRegistry {
    fn reserve(&mut self) -> ModuleHandle {
        let h = ModuleHandle(self.next_handle);
        self.next_handle += 1;
        h
    }

    fn register(&mut self, handle: ModuleHandle, inst: ModuleInstance) {
        match self.map.entry(handle) {
            Entry::Vacant(e) => {
                e.insert(inst);
            }
            Entry::Occupied(_) => panic!("Handle taken"),
        }
    }

    fn get_instance(&self, handle: ModuleHandle) -> &ModuleInstance {
        self.map.get(&handle).unwrap()
    }
}

#[derive(Debug, Default)]
pub struct Runtime {
    call_stack: Vec<Frame>,
    value_stack: Vec<Value>,

    store: Store,
    module_registry: ModuleRegistry,
}

#[derive(Debug, Clone, Copy)]
pub enum Value {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    NullRef,
    FuncRef(usize),
    ExternRef(usize),
}

impl From<ValType> for Value {
    fn from(value: ValType) -> Self {
        match value {
            ValType::I32 => Value::I32(0),
            ValType::I64 => Value::I64(0),
            ValType::F32 => Value::F32(0.0),
            ValType::F64 => Value::F64(0.0),
            ValType::V128 => unimplemented!(),
        }
    }
}

#[derive(Debug)]
pub enum StackEntry {
    Value(Value),
    Label,
    Activation(Frame),
}

#[derive(Debug)]
pub struct Frame {
    arity: u32,
    funcaddr: FuncAddr,
    locals: Vec<Value>,
    pc: isize,
    sp: usize,
}

impl Runtime {
    fn instantiate_module(&mut self, module: &Module) -> ModuleHandle {
        let h = self.module_registry.reserve();
        let mut mi = ModuleInstance::default();

        // Handle optional sections - minimal modules may have none
        if let Some(typesec) = &module.types {
            mi.types = typesec.clone();
        }

        // Only process functions if we have both function and code sections
        if let (Some(funcsec), Some(codesec), Some(typesec)) =
            (&module.functions, &module.codes, &module.types)
        {
            for (idx, typeidx) in funcsec.iter().enumerate() {
                let typeidx = *typeidx;

                let locals = {
                    let mut v = vec![];
                    for l in codesec[idx].locals.as_slice() {
                        v.extend(repeat_n(l.valtype, l.count as usize));
                    }
                    v
                };
                let func = Func {
                    typeidx,
                    locals,
                    body: codesec[idx].body.clone(),
                };

                let funcinst = FuncInstance {
                    ftype: typesec[typeidx as usize].clone(),
                    module: h,
                    func,
                };

                mi.funcaddrs.push(self.store.funcs.len().into());
                self.store.funcs.push(funcinst);
            }
        }

        // Only process exports if we have them
        if let Some(exports) = &module.exports {
            for export in exports {
                let funcaddr = mi.funcaddrs[export.index as usize];
                mi.exports
                    .insert(export.name.clone(), ExternVal::Func(funcaddr));
            }
        }

        self.module_registry.register(h, mi);
        h
    }

    pub fn invoke(&mut self, module: ModuleHandle, fn_name: &str, fn_args: &[Value]) -> Vec<Value> {
        let mi = self.module_registry.get_instance(module);
        let funcaddr = mi.exports.get(fn_name).unwrap().try_into().unwrap();
        let arity = self.store.get_func(funcaddr).ftype.results.len();

        self.value_stack.extend_from_slice(fn_args);

        self.call(funcaddr);
        self.execute();

        let idx = self.value_stack.len() - arity;
        self.value_stack.split_off(idx)
    }

    fn call(&mut self, funcaddr: FuncAddr) {
        let func_instance = self.store.get_func(funcaddr);
        let n_args = func_instance.ftype.params.len();
        let arity = func_instance.ftype.results.len() as u32;
        let sp = self.value_stack.len() - n_args;

        let mut locals: Vec<Value> = self.value_stack.split_off(sp);
        locals.extend(
            func_instance
                .func
                .locals
                .clone()
                .into_iter()
                .map(Into::<Value>::into),
        );

        self.call_stack.push(Frame {
            arity,
            funcaddr,
            locals,
            pc: -1,
            sp,
        });
    }

    fn execute(&mut self) {
        while let Some(frame) = self.call_stack.last_mut() {
            let func_inst = self.store.get_func(frame.funcaddr);
            let instrs = &func_inst.func.body;

            frame.pc += 1;
            if let Some(inst) = instrs.get(frame.pc as usize) {
                match inst {
                    Instruction::Nop => continue,
                    Instruction::If(_) => {
                        let cond = self.value_stack.pop().unwrap();
                        match cond {
                            Value::I32(0) => {
                                frame.pc += instrs[frame.pc as usize..]
                                    .iter()
                                    .position(|&x| x == Instruction::Else || x == Instruction::End)
                                    .unwrap() as isize;
                            }

                            Value::I32(_) => continue,
                            _ => unreachable!(),
                        };
                    }
                    Instruction::Else => {
                        frame.pc += instrs[frame.pc as usize..]
                            .iter()
                            .position(|&x| x == Instruction::End)
                            .unwrap() as isize;
                    }
                    Instruction::End => {
                        if frame.pc as usize == instrs.len() - 1 {
                            let results = {
                                let idx = self.value_stack.len() - frame.arity as usize;
                                self.value_stack.split_off(idx)
                            };
                            self.value_stack.truncate(frame.sp);
                            self.value_stack.extend(results);
                            self.call_stack.pop();
                        }
                    }
                    Instruction::Call(idx) => {
                        let mi = self.module_registry.get_instance(func_inst.module);
                        let funcaddr = mi.funcaddrs[*idx as usize];
                        self.call(funcaddr);
                    }
                    Instruction::LocalGet(idx) => {
                        let v = frame.locals[*idx as usize];
                        self.value_stack.push(v)
                    }
                    Instruction::LocalSet(_) => todo!(),
                    Instruction::LocalTee(_) => todo!(),
                    Instruction::I32Const(v) => {
                        self.value_stack.push(Value::I32(*v));
                    }
                    Instruction::I32LeS => {
                        let rhs = self.value_stack.pop().unwrap();
                        let lhs = self.value_stack.pop().unwrap();
                        let res = match (lhs, rhs) {
                            (Value::I32(a), Value::I32(b)) => a <= b,
                            _ => unreachable!(),
                        };
                        self.value_stack.push(Value::I32(res as i32));
                    }
                    Instruction::I32Add => {
                        let rhs = self.value_stack.pop().unwrap();
                        let lhs = self.value_stack.pop().unwrap();
                        let res = match (lhs, rhs) {
                            (Value::I32(a), Value::I32(b)) => a + b,
                            _ => unreachable!(),
                        };
                        self.value_stack.push(Value::I32(res));
                    }
                    Instruction::I32Sub => {
                        let rhs = self.value_stack.pop().unwrap();
                        let lhs = self.value_stack.pop().unwrap();
                        let res = match (lhs, rhs) {
                            (Value::I32(a), Value::I32(b)) => a - b,
                            _ => unreachable!(),
                        };
                        self.value_stack.push(Value::I32(res));
                    }
                    Instruction::I32Mul => {
                        let rhs = self.value_stack.pop().unwrap();
                        let lhs = self.value_stack.pop().unwrap();
                        let res = match (lhs, rhs) {
                            (Value::I32(a), Value::I32(b)) => a * b,
                            _ => unreachable!(),
                        };
                        self.value_stack.push(Value::I32(res));
                    }
                }
            }
        }
    }

    /// Decode and instantiate a module from bytes
    pub fn load_module(&mut self, bytes: &[u8]) -> std::result::Result<ModuleHandle, Error> {
        let module = decode_module(bytes.to_vec())?;
        let handle = self.instantiate_module(&module);
        Ok(handle)
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Malformed(SectionError),
    Invalid,
    Trap,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Malformed(e) => write!(f, "malformed module: {}", e),
            Error::Invalid => write!(f, "invalid module"),
            Error::Trap => write!(f, "trap"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed(e) => Some(e),
            _ => None,
        }
    }
}

/// Decode a module from bytes (parsing only, no instantiation)
fn decode_module(bytes: Vec<u8>) -> std::result::Result<Module, Error> {
    let mut m = Module::from_bytes(bytes);

    let mut r = ModuleReader::from_module(&m);
    r.read_preamble().map_err(|e| {
        Error::Malformed(SectionError {
            kind: sections::SectionErrorKind::Preamble(e),
            info: SectionInfo {
                id: SectionId::Custom,
                start: 0,
                end: 0,
                size: 0,
            },
            idx: None,
        })
    })?;
    m.sections = r.read_toc().map_err(|e| {
        Error::Malformed(SectionError {
            kind: sections::SectionErrorKind::Toc(e),
            info: SectionInfo {
                id: SectionId::Custom,
                start: 0,
                end: 0,
                size: 0,
            },
            idx: None,
        })
    })?;

    for item in m.sections.iter() {
        let start = item.start as usize;
        let end = item.end as usize;

        let mut reader = Reader::from_bytes(&m.bytes[..end], start);

        match item.id {
            SectionId::Custom => {}
            SectionId::Type => {
                m.types = Some(read_type_section(&mut reader, *item).map_err(Error::Malformed)?);
            }
            SectionId::Import => {}
            SectionId::Function => {
                m.functions =
                    Some(read_function_section(&mut reader, *item).map_err(Error::Malformed)?);
            }
            SectionId::Table => {}
            SectionId::Memory => {}
            SectionId::Global => {}
            SectionId::Export => {
                m.exports =
                    Some(read_export_section(&mut reader, *item).map_err(Error::Malformed)?);
            }
            SectionId::Start => {}
            SectionId::Element => {}
            SectionId::Code => {
                m.codes = Some(read_code_section(&mut reader, *item).map_err(Error::Malformed)?);
            }
            SectionId::Data => {}
            SectionId::DataCount => {}
            SectionId::Unknown(_) => {}
        };
    }

    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_minimal_module() -> anyhow::Result<()> {
        let bytes = b"\0asm\x01\x00\x00\x00".to_vec();

        let _ = decode_module(bytes)?;

        Ok(())
    }

    #[test]
    fn test_decode_type_section() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x01\x06",                 // Type section(1), 6 bytes
            b"\x01\x60\x01\x7f\x01\x7f", // 1 function, (i32) -> i32
        ]
        .concat();

        let _ = decode_module(bytes)?;

        Ok(())
    }

    #[test]
    fn test_decode_invalid_section() -> anyhow::Result<()> {
        let bytes = [
            b"\0asm\x01\x00\x00\x00" as &[u8],
            b"\x0f\x06",
            b"\x01\x60\x01\x7f\x01\x7f",
        ]
        .concat();

        let m = decode_module(bytes);
        assert!(m.is_err());

        Ok(())
    }
}
