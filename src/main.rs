use std::{
    collections::{hash_map::Entry, HashMap},
    fs,
    io::{Seek, SeekFrom},
    iter::repeat_n,
};

mod leb128;
mod reader;

use crate::{
    instructions::Instruction,
    reader::ReadErrorKind,
    sections::{
        read_code_section, read_export_section, read_function_section, read_type_section,
        CodeSection, ExportSection, FunctionSection, SectionId, TypeSection,
    },
    types::{FuncType, TypeIdx, ValType},
};
use reader::{ReadError, Reader, Result};

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

    fn read_preamble(&mut self) -> Result<()> {
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

    fn read_toc(&mut self) -> Result<Vec<SectionInfo>> {
        let mut v = Vec::new();
        while self.reader.has_data_left()? {
            let id: SectionId = self.reader.read_u8()?.into();
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
    // Vec128,
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
    Label, // TODO
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

        let codesec = module.codes.as_ref().unwrap();
        let typesec = module.types.as_ref().unwrap();

        // Populate Types
        mi.types = typesec.clone();

        // Populate Functions
        for (idx, typeidx) in module.functions.as_ref().unwrap().iter().enumerate() {
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

        // Populate Exports
        for export in module.exports.as_ref().unwrap() {
            // Map address from module to store
            let funcaddr = mi.funcaddrs[export.index as usize];

            // FIXME We assume only functions are exported
            // ExportInstance from Entry ?
            mi.exports
                .insert(export.name.clone(), ExternVal::Func(funcaddr));
        }

        self.module_registry.register(h, mi);
        h
    }

    fn invoke(&mut self, module: ModuleHandle, fn_name: &str, fn_args: &[Value]) -> Vec<Value> {
        let mi = self.module_registry.get_instance(module);
        let funcaddr = mi.exports.get(fn_name).unwrap().try_into().unwrap();
        let arity = self.store.get_func(funcaddr).ftype.results.len();

        self.value_stack.extend_from_slice(fn_args);

        self.call(funcaddr);

        // TODO If error, restore stacks to previous state
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
                    Instruction::End => {
                        // take results
                        let results = {
                            let idx = self.value_stack.len() - frame.arity as usize;
                            self.value_stack.split_off(idx)
                        };

                        // unwind
                        self.value_stack.truncate(frame.sp);

                        // push results
                        self.value_stack.extend(results);

                        // pop frame
                        self.call_stack.pop();
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
                    Instruction::I32Const(_) => todo!(),
                    Instruction::I32Add => {
                        let rhs = self.value_stack.pop().unwrap();
                        let lhs = self.value_stack.pop().unwrap();

                        let res = match (lhs, rhs) {
                            (Value::I32(a), Value::I32(b)) => a + b,
                            _ => unreachable!(),
                        };

                        self.value_stack.push(Value::I32(res));
                    }
                }
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input.wasm>", args[0]);
        std::process::exit(1);
    }
    let bytes = fs::read(&args[1])?;
    let mut m = Module::from_bytes(bytes);

    let mut r = ModuleReader::from_module(&m);

    r.read_preamble()?;
    m.sections = r.read_toc()?;

    println!("Sections:");
    for s in &m.sections {
        println!(
            "{:>15}, start={:#010x}, end={:#010x} (size={:#010x})",
            s.id, s.start, s.end, s.size
        );
    }

    for item in m.sections.iter() {
        let start = item.start as usize;
        let end = item.end as usize;

        let mut reader = Reader::from_bytes(&m.bytes[..end], start);

        match item.id {
            SectionId::Custom => todo!(),
            SectionId::Type => {
                m.types = Some(read_type_section(&mut reader, *item)?);
            }
            SectionId::Import => todo!(),
            SectionId::Function => {
                m.functions = Some(read_function_section(&mut reader, *item)?);
            }
            SectionId::Table => todo!(),
            SectionId::Memory => todo!(),
            SectionId::Global => todo!(),
            SectionId::Export => {
                m.exports = Some(read_export_section(&mut reader, *item)?);
            }
            SectionId::Start => todo!(),
            SectionId::Element => todo!(),
            SectionId::Code => {
                m.codes = Some(read_code_section(&mut reader, *item)?);
            }
            SectionId::Data => todo!(),
            SectionId::DataCount => todo!(),
            SectionId::Unknown(_) => todo!(),
        };
    }

    // Execution
    let mut runtime = Runtime::default();
    let mh = runtime.instantiate_module(&m);
    let r = runtime.invoke(mh, "add", &[Value::I32(10), Value::I32(2)]);
    dbg!(r);

    Ok(())
}
