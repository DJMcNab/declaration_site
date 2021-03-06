use std::time::Instant;

use declaration_site::declaration_of;

fn main() {
    let now = Instant::now();
    let declaration =
        declaration_of(&function_to_find).expect("Should have gotten declaration site");
    println!(
        "Found {declaration} in {elapsed:?}",
        elapsed = now.elapsed()
    );
    // Ensure that the function is linked
    function_to_find();
}

#[inline(never)]
fn function_to_find() {}
