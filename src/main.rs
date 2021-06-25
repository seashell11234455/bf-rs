use crate::interpreter::Interpreter;
use std::{
    fs::File,
    io::{
        self,
        Read,
    },
    time::SystemTime,
};

mod interpreter;

fn main() {
    let mut reader = File::open("./samples/bfbf.bf").expect("Cannot open file");
    let mut src = String::new();
    reader.read_to_string(&mut src).expect("Fail to read file");
    let interpreter = Interpreter::new(src.chars()).unwrap();
    println!("{:?}", interpreter);
    let mut reader = File::open("./samples/bottles.bf").expect("Cannot open file");
    let mut input = String::new();
    reader.read_to_string(&mut input).expect("Fail to read file");
    input.extend("\x0062500\n".chars());
    let now = SystemTime::now();
    interpreter.compile()(&input.as_bytes(), &io::stdout());
    println!("Time cost: {}ms", SystemTime::now().duration_since(now).unwrap().as_millis());
}
