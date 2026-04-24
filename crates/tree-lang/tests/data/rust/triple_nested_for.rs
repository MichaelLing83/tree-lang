fn triple_nested() {
    for _ in 0..1 {
        let i = 1;
        for _ in 0..1 {
            for _ in 0..1 {}
        }
    }
}
