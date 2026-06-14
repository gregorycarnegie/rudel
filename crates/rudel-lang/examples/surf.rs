fn main() {
    let r = rudel_lang::reference();
    println!("FUNCTIONS {}", r.functions.join(" "));
    println!("METHODS {}", r.methods.join(" "));
}
