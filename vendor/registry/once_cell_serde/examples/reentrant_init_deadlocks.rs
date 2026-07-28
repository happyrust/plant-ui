fn main() {
    let cell = once_cell_serde::sync::OnceCell::<u32>::new();
    cell.get_or_init(|| {
        cell.get_or_init(|| 1);
        2
    });
}

/// Dummy test to make it seem hang when compiled as `--test`
/// See https://github.com/matklad/once_cell/issues/79
#[test]
fn dummy_test() {
    std::thread::sleep(std::time::Duration::from_secs(4));
}
