use skeleton::add;

fn main() {
    println!("Hello, world!");
    println!("{} + {} = {}", 5, 6, add(5, 6));
    let daya: Vec<i32> = vec![
        1, 3, 5, 7, 89, 56, 3, 2, 123, 7, 88, 65, 43, 2, 35, 677, 788, 0,
    ];
    println!("daya: {:?}", daya.iter().sum::<i32>());
}
