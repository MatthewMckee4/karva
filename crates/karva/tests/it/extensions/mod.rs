mod approx;
pub mod fixtures;
pub mod functions;
pub mod snapshot;
pub mod tags;

fn get_auto_use_kw(framework: &str) -> &str {
    match framework {
        "pytest" => "autouse",
        "karva" => "auto_use",
        _ => panic!("Invalid framework"),
    }
}

fn get_parametrize_function(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.parametrize",
        "karva" => "karva.tags.parametrize",
        _ => panic!("Invalid framework"),
    }
}
