use std::{
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

#[derive(Debug)]
pub struct FuncAddr(usize);

#[derive(Debug)]
pub enum ExternVal {
    Func(FuncAddr),
    Table(usize),
    Mem(usize),
    Global(usize),
}

#[derive(Debug)]
pub struct ExportInstance {
    name: String,
    value: ExternVal,
}

#[derive(Debug, Default)]
pub struct ModuleInstance {
    types: Vec<FuncType>,
    funcaddrs: Vec<usize>,
    exports: Vec<ExportInstance>,
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
    module: usize,
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

#[derive(Debug, Default)]
pub struct Runtime {
    stack: Vec<StackEntry>,
    store: Store,
    modules: Vec<ModuleInstance>,
}

#[derive(Debug)]
pub enum Values {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    // Vec128,
    NullRef,
    FuncRef(usize),
    ExternRef(usize),
}

#[derive(Debug)]
pub enum StackEntry {
    Value(Values),
    Label, // TODO
    Activation(Frame),
}

#[derive(Debug)]
pub struct Frame {
    arity: u32,
    funcaddr: FuncAddr,
    pc: isize,
    locals: Vec<Values>,
}

impl Runtime {
    fn add_module(&mut self, module: &Module) {
        self.modules.push(ModuleInstance::default());

        let midx = self.modules.len() - 1;
        let mi = self.modules.last_mut().unwrap();

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
                module: midx,
                func,
            };

            mi.funcaddrs.push(self.store.funcs.len());
            self.store.funcs.push(funcinst);
        }

        // Populate Exports
        for export in module.exports.as_ref().unwrap() {
            // Map address from module to store
            let funcaddr = mi.funcaddrs[export.index as usize];

            // FIXME We assume only functions are exported
            // ExportInstance from Entry ?
            mi.exports.push(ExportInstance {
                name: export.name.clone(),
                value: ExternVal::Func(funcaddr),
            });
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
    runtime.add_module(&m);

    dbg!(runtime);

    Ok(())
}
