use std::{
    mem,
    io::{Read, Write},
};
use dynasmrt::{dynasm, DynasmApi, DynasmLabelApi, x64::Assembler};
use std::collections::HashMap;

#[derive(Debug, Copy, Clone)]
enum Token {
    Add(i16, i32),
    Mul(i16, i32, i32),
    AddTo(i32, i32),
    Clear(i32),
    Shift(i32),
    LoopBegin(i32),
    LoopEnd(i32),
    Input(i32),
    Output(i32),
    End,
}

#[derive(Debug)]
pub struct Interpreter {
    inst: Vec<Token>,
}

impl Interpreter {
    pub fn new<I: IntoIterator<Item=char>>(stream: I) -> Result<Self, &'static str> {
        let mut inst = Vec::new();
        let mut depth = 0;
        let mut shift = 0;
        let mut begin = 0;
        let mut mp = HashMap::new();
        for c in stream.into_iter() {
            match match c {
                '+' => Token::Add(1, 0),
                '-' => Token::Add(-1, 0),
                '>' => Token::Shift(1),
                '<' => Token::Shift(-1),
                ',' => Token::Input(0),
                '.' => Token::Output(0),
                '[' => {
                    depth += 1;
                    Token::LoopBegin(0)
                }
                ']' => {
                    depth -= 1;
                    if depth < 0 {
                        return Err("[ missing.");
                    }
                    Token::LoopEnd(0)
                }
                _ => continue,
            } {
                Token::Add(n, _) => {
                    match mp.get_mut(&shift) {
                        None => { mp.insert(shift, n); }
                        Some(add) => { *add += n; }
                    }
                }
                Token::Shift(n) => {
                    shift += n;
                }
                Token::Output(_) => {
                    if let Some(add) = mp.get(&shift) {
                        inst.push(Token::Add(*add, shift));
                        mp.remove(&shift);
                    }
                    inst.push(Token::Output(shift));
                }
                Token::Input(_) => {
                    if let Some(_) = mp.get(&shift) {
                        mp.remove(&shift);
                    }
                    inst.push(Token::Input(shift));
                }
                Token::LoopBegin(_) => {
                    for (shift, add) in &mp {
                        if *add != 0 {
                            inst.push(Token::Add(*add, *shift));
                        }
                    }
                    mp.clear();
                    if shift != 0 {
                        inst.push(Token::Shift(shift));
                        shift = 0;
                    }
                    inst.push(Token::LoopBegin(0));
                    begin = inst.len();
                }
                Token::LoopEnd(_) => {
                    if inst.len() == begin && shift == 0 && &mp.get(&0) == &Some(&-1) {
                        inst.pop().unwrap();
                        if let Some(Token::Shift(prev_shift)) = inst.last() {
                            shift = *prev_shift;
                            inst.pop();
                        }
                        mp.remove(&0);
                        for (offset, add) in &mp {
                            inst.push(match *add {
                                0 => continue,
                                1 => Token::AddTo(*offset + shift, shift),
                                _ => Token::Mul(*add, *offset + shift, shift),
                            });
                        }
                        inst.push(Token::Clear(shift));
                        mp.clear();
                        begin = 0;
                    } else {
                        for (shift, add) in &mp {
                            if *add != 0 {
                                inst.push(Token::Add(*add, *shift));
                            }
                        }
                        if shift != 0 {
                            inst.push(Token::Shift(shift));
                            shift = 0;
                        }
                        inst.push(Token::LoopEnd(0));
                        mp.clear();
                    }
                }
                _ => {}
            }
        }
        if depth > 0 {
            return Err("] missing.");
        }
        inst.push(Token::End);
        Ok(Self {
            inst
        }.build_jump_addr())
    }

    fn build_jump_addr(self) -> Self {
        let mut opt = Vec::new();
        let mut stack = Vec::new();
        for i in 0..self.inst.len() {
            match self.inst[i] {
                Token::LoopBegin(_) => {
                    stack.push(i);
                    opt.push(Token::LoopBegin(0));
                }
                Token::LoopEnd(_) => {
                    let pos = stack.pop().unwrap();
                    let shift = i as i32 - pos as i32;
                    opt[pos] = Token::LoopBegin(shift + 1);
                    opt.push(Token::LoopEnd(1 - shift));
                }
                tk => opt.push(tk),
            }
        }
        Self {
            inst: opt,
        }
    }

    #[allow(dead_code)]
    pub fn run(&self, reader: &mut dyn Read, writer: &mut dyn Write) {
        let mut i = 0;
        let mut pos = 0;
        let mut buffer = [0u8; 0xffff];
        loop {
            match self.inst[i as usize] {
                Token::Add(n, shift) => {
                    let rhs = buffer[(pos + shift) as usize] as i16;
                    buffer[(pos + shift) as usize] = (n + rhs) as u8;
                }
                Token::Mul(n, shift, base) => {
                    let rhs = buffer[(pos + shift) as usize] as i16;
                    let mul = buffer[(pos + base) as usize] as i16;
                    buffer[(pos + shift) as usize] = (n * mul + rhs) as u8;
                }
                Token::AddTo(to, from) => {
                    let to_n = buffer[(to + pos) as usize] as i16;
                    let from_n = buffer[(pos + from) as usize] as i16;
                    buffer[(to + pos) as usize] = (from_n + to_n) as u8;
                }
                Token::Clear(shift) => buffer[(pos + shift) as usize] = 0,
                Token::Shift(shift) => pos += shift,
                Token::LoopBegin(label) => if buffer[pos as usize] == 0 {
                    i += label;
                    continue;
                }
                Token::LoopEnd(label) => if buffer[pos as usize] != 0 {
                    i += label;
                    continue;
                }
                Token::Input(shift) => {
                    let mut buf = [0u8];
                    reader.read(&mut buf).unwrap();
                    buffer[(pos + shift) as usize] = buf[0];
                }
                Token::Output(shift) => {
                    writer.write(&[buffer[(pos + shift) as usize]]).unwrap();
                }
                Token::End => break,
            }
            i += 1;
        }
    }

    #[allow(dead_code)]
    pub fn compile(&self) -> Box<dyn Fn(&dyn Read, &dyn Write)> {
        let mut ops = Assembler::new().unwrap();
        let start = ops.offset();
        let mut labels = Vec::new();
        dynasm!(ops
            ; push rbp
            ; mov rbp, rsp
            ; sub rsp, 0x30
            ; mov [rsp + 0x18], rdx
            ; mov [rsp + 0x10], r8
            ; mov rdx, 0
        );
        for i in 0..self.inst.len() {
            match self.inst[i] {
                Token::Add(n, shift) => {
                    dynasm!(ops
                        ; add BYTE [rcx + rdx + shift as _], n as _
                    );
                }
                Token::Mul(n, shift, base) => {
                    dynasm!(ops
                        ; mov al, BYTE [rcx + rdx + base as _]
                        ; mov r8b, n as _
                        ; mul r8b
                        ; add BYTE [rcx + rdx + shift as _], al
                    );
                }
                Token::AddTo(to, from) => {
                    dynasm!(ops
                        ; mov al, BYTE [rcx + rdx + from as _]
                        ; add BYTE [rcx + rdx + to as _], al
                    );
                }
                Token::Clear(shift) => {
                    dynasm!(ops
                        ; mov BYTE [rcx + rdx + shift as _], 0
                    );
                }
                Token::Shift(shift) => {
                    dynasm!(ops
                        ; add rdx, shift as _
                    );
                }
                Token::LoopBegin(_) => {
                    let backward_label = ops.new_dynamic_label();
                    let forward_label = ops.new_dynamic_label();
                    labels.push((backward_label, forward_label));
                    dynasm!(ops
                        ; cmp BYTE [rcx + rdx], 0
                        ; jz =>forward_label
                        ;=>backward_label
                    );
                }
                Token::LoopEnd(_) => {
                    let (backward_label, forward_label) = labels.pop().unwrap();
                    dynasm!(ops
                        ; cmp BYTE [rcx + rdx], 0
                        ; jnz =>backward_label
                        ;=>forward_label
                    );
                }
                Token::Input(shift) => {
                    dynasm!(ops
                        ; mov [rsp + 0x28], rcx
                        ; mov [rsp + 0x20], rdx
                        ; mov rcx, [rsp + 0x18]
                        ; mov rdx, [rsp + 0x10]
                        ; mov rax, QWORD Self::getchar as _
                        ; call rax
                        ; mov rdx, [rsp + 0x20]
                        ; mov rcx, [rsp + 0x28]
                        ; mov [rcx + rdx + shift as _], rax
                    );
                }
                Token::Output(shift) => {
                    dynasm!(ops
                        ; mov [rsp + 0x28], rcx
                        ; mov [rsp + 0x20], rdx
                        ; mov cl, [rcx + rdx + shift as _]
                        ; mov rdx, [rsp + 0x10]
                        ; mov rax, QWORD Self::putchar as _
                        ; call rax
                        ; mov rdx, [rsp + 0x20]
                        ; mov rcx, [rsp + 0x28]
                    );
                }
                Token::End => {
                    dynasm!(ops
                        ; mov rsp, rbp
                        ; pop rbp
                        ; ret
                    );
                }
            }
        }
        let buf = ops.finalize().unwrap();
        Box::new(move |reader: &dyn Read, writer: &dyn Write| {
            let mut buffer = [0u8; 0xffff];
            let f: fn(_, _, _) = unsafe { mem::transmute(buf.ptr(start)) };
            let raw_reader = Box::into_raw(Box::new(reader));
            let raw_writer = Box::into_raw(Box::new(writer));
            f(buffer.as_mut_ptr(), raw_reader, raw_writer);
            unsafe {
                Box::from_raw(raw_reader);
                Box::from_raw(raw_writer);
            }
        })
    }

    unsafe fn putchar(char: u8, writer: *mut &mut dyn Write) {
        let buf = [char as u8];
        let writer = &mut **writer;
        writer.write(&buf).unwrap();
        writer.flush().unwrap();
    }

    unsafe fn getchar(reader: *mut &mut dyn Read) -> u8 {
        let mut buf = [0];
        (**reader).read(&mut buf).unwrap();
        buf[0]
    }
}