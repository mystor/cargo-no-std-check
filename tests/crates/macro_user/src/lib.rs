#![no_std]

#[derive(macro_impl::Thingy)]
struct Target;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
