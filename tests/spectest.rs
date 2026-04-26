use wasmrs::runtime::Runtime;
use wasmrs::runtime::value::Value;
use wasmrs::{
    FuncType, GlobalType, Limit, MemType, MutabilityFlag, Ref, RefType, TableType, ValType,
};

pub fn setup_spectest(runtime: &mut Runtime) {
    // Print functions
    let _ = runtime.register_host_fn(
        "spectest",
        "print",
        FuncType {
            params: vec![],
            results: vec![],
        },
        |_| Ok(vec![]),
    );
    let _ = runtime.register_host_fn(
        "spectest",
        "print_i32",
        FuncType {
            params: vec![ValType::I32],
            results: vec![],
        },
        |_: &[Value]| Ok(vec![]),
    );
    let _ = runtime.register_host_fn(
        "spectest",
        "print_i64",
        FuncType {
            params: vec![ValType::I64],
            results: vec![],
        },
        |_: &[Value]| Ok(vec![]),
    );
    let _ = runtime.register_host_fn(
        "spectest",
        "print_f32",
        FuncType {
            params: vec![ValType::F32],
            results: vec![],
        },
        |_: &[Value]| Ok(vec![]),
    );
    let _ = runtime.register_host_fn(
        "spectest",
        "print_f64",
        FuncType {
            params: vec![ValType::F64],
            results: vec![],
        },
        |_: &[Value]| Ok(vec![]),
    );

    let _ = runtime.register_host_fn(
        "spectest",
        "print_i32_f32",
        FuncType {
            params: vec![ValType::I32, ValType::F32],
            results: vec![],
        },
        |_: &[Value]| Ok(vec![]),
    );

    let _ = runtime.register_host_fn(
        "spectest",
        "print_f64_f64",
        FuncType {
            params: vec![ValType::F64, ValType::F64],
            results: vec![],
        },
        |_: &[Value]| Ok(vec![]),
    );

    // Globals
    let _ = runtime.register_host_global(
        "spectest",
        "global_i32",
        GlobalType {
            type_: ValType::I32,
            mutflag: MutabilityFlag::Const,
        },
        Value::I32(666),
    );
    let _ = runtime.register_host_global(
        "spectest",
        "global_i64",
        GlobalType {
            type_: ValType::I64,
            mutflag: MutabilityFlag::Const,
        },
        Value::I64(666),
    );
    let _ = runtime.register_host_global(
        "spectest",
        "global_f32",
        GlobalType {
            type_: ValType::F32,
            mutflag: MutabilityFlag::Const,
        },
        Value::F32(666.6),
    );
    let _ = runtime.register_host_global(
        "spectest",
        "global_f64",
        GlobalType {
            type_: ValType::F64,
            mutflag: MutabilityFlag::Const,
        },
        Value::F64(666.6),
    );

    // Tables
    let _ = runtime.register_host_table(
        "spectest",
        "table",
        TableType {
            elemtype: RefType::Func,
            limit: Limit {
                min: 10,
                max: Some(20),
            },
        },
        Ref::Null(RefType::Func),
    );

    // Memory
    let _ = runtime.register_host_memory(
        "spectest",
        "memory",
        MemType(Limit {
            min: 1,
            max: Some(2),
        }),
    );
}
