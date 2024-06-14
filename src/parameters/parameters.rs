use std::io::{self, Write};

pub fn watch(prompt: &str) -> String {
    println!("{}", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("failed to read input");
    let received: String = input.trim().to_string().parse().expect("failed to parse");
    received
}



#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn it_returns_strings() {
        let re = watch("Write useful");
        assert_eq!(re, "usefuls");
    }
}