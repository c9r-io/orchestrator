pub fn out_line(message: impl AsRef<str>) {
    println!("{}", message.as_ref());
}

pub fn err_line(message: impl AsRef<str>) {
    eprintln!("{}", message.as_ref());
}
