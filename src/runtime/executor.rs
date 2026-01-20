use std::{
    ops::{BitAnd, BitOr, BitXor},
    result,
};

use crate::{
    Error,
    instructions::Instruction,
    runtime::{Runtime, value::Value},
};

impl Runtime {
    fn unary_op_i32<F, R>(&mut self, unop: F)
    where
        F: FnOnce(i32) -> R,
        R: Into<i32>,
    {
        let lhs = self.value_stack.pop().unwrap();
        let res = match lhs {
            Value::I32(a) => unop(a).into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res));
    }

    fn binary_op_i32<F, R>(&mut self, binop: F)
    where
        F: FnOnce(i32, i32) -> R,
        R: Into<i32>,
    {
        let rhs = self.value_stack.pop().unwrap();
        let lhs = self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::I32(a), Value::I32(b)) => binop(a, b).into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res));
    }

    fn try_binary_op_i32<F, R>(&mut self, binop: F) -> result::Result<(), Error>
    where
        F: FnOnce(i32, i32) -> Option<R>,
        R: Into<i32>,
    {
        let rhs = self.value_stack.pop().unwrap();
        let lhs = self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::I32(a), Value::I32(b)) => binop(a, b).ok_or(Error::Trap)?.into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res));
        Ok(())
    }

    fn binary_op_u32<F, R>(&mut self, binop: F)
    where
        F: FnOnce(u32, u32) -> R,
        R: Into<u32>,
    {
        let rhs = self.value_stack.pop().unwrap();
        let lhs = self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::I32(a), Value::I32(b)) => binop(a as u32, b as u32).into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res as i32));
    }

    fn try_binary_op_u32<F, R>(&mut self, binop: F) -> Result<(), Error>
    where
        F: FnOnce(u32, u32) -> Option<R>,
        R: Into<u32>,
    {
        let rhs = self.value_stack.pop().unwrap();
        let lhs = self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::I32(a), Value::I32(b)) => binop(a as u32, b as u32).ok_or(Error::Trap)?.into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res as i32));
        Ok(())
    }

    pub(super) fn execute(&mut self) -> Result<(), Error> {
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
                    Instruction::Drop => {
                        self.value_stack.pop();
                    }
                    Instruction::LocalGet(idx) => {
                        let v = frame.locals[*idx as usize];
                        self.value_stack.push(v)
                    }
                    Instruction::LocalSet(_) => todo!(),
                    Instruction::LocalTee(_) => todo!(),

                    Instruction::I32Const(v) => self.value_stack.push(Value::I32(*v)),
                    Instruction::I64Const(v) => self.value_stack.push(Value::I64(*v)),

                    // Comparison ops
                    Instruction::I32Eqz => self.unary_op_i32(|a| a == 0),
                    Instruction::I32Eq => self.binary_op_i32(|a, b| a == b),
                    Instruction::I32Ne => self.binary_op_i32(|a, b| a != b),
                    Instruction::I32LtS => self.binary_op_i32(|a, b| a < b),
                    Instruction::I32LtU => self.binary_op_u32(|a, b| a < b),
                    Instruction::I32GtS => self.binary_op_i32(|a, b| a > b),
                    Instruction::I32GtU => self.binary_op_u32(|a, b| a > b),
                    Instruction::I32LeS => self.binary_op_i32(|a, b| a <= b),
                    Instruction::I32LeU => self.binary_op_u32(|a, b| a <= b),
                    Instruction::I32GeS => self.binary_op_i32(|a, b| a >= b),
                    Instruction::I32GeU => self.binary_op_u32(|a, b| a >= b),

                    // Unary ops
                    Instruction::I32Clz => self.unary_op_i32(|a| a.leading_zeros() as i32),
                    Instruction::I32Ctz => self.unary_op_i32(|a| a.trailing_zeros() as i32),
                    Instruction::I32Popcnt => self.unary_op_i32(|a| a.count_ones() as i32),

                    // Arithmetic ops
                    Instruction::I32Add => self.binary_op_i32(|a, b| a.wrapping_add(b)),
                    Instruction::I32Sub => self.binary_op_i32(|a, b| a.wrapping_sub(b)),
                    Instruction::I32Mul => self.binary_op_i32(|a, b| a.wrapping_mul(b)),
                    Instruction::I32DivS => self.try_binary_op_i32(|a, b| a.checked_div(b))?,
                    Instruction::I32DivU => self.try_binary_op_u32(|a, b| a.checked_div(b))?,
                    Instruction::I32RemS => self.try_binary_op_i32(|a, b| {
                        if b == 0 {
                            None
                        } else {
                            Some(a.wrapping_rem(b))
                        }
                    })?,

                    Instruction::I32RemU => self.try_binary_op_u32(|a, b| {
                        if b == 0 {
                            None
                        } else {
                            Some(a.wrapping_rem(b))
                        }
                    })?,
                    Instruction::I32And => self.binary_op_i32(BitAnd::bitand),
                    Instruction::I32Or => self.binary_op_i32(BitOr::bitor),
                    Instruction::I32Xor => self.binary_op_i32(BitXor::bitxor),
                    Instruction::I32Shl => self.binary_op_i32(|a, b| a.wrapping_shl(b as u32)),
                    Instruction::I32ShrS => self.binary_op_i32(|a, b| a.wrapping_shr(b as u32)),
                    Instruction::I32ShrU => self.binary_op_u32(|a, b| a.wrapping_shr(b)),
                    Instruction::I32Rotl => self.binary_op_i32(|a, b| a.rotate_left(b as u32)),
                    Instruction::I32Rotr => self.binary_op_i32(|a, b| a.rotate_right(b as u32)),

                    // Sign extension ops
                    Instruction::I32Extend8S => self.unary_op_i32(|a| a as i8),
                    Instruction::I32Extend16S => self.unary_op_i32(|a| a as i16),

                    // Ref
                    Instruction::RefNull(rt) => self.value_stack.push(Value::NullRef(*rt)),
                    Instruction::RefFunc(fi) => self.value_stack.push(Value::FuncRef(*fi)),
                }
            }
        }
        Ok(())
    }
}
