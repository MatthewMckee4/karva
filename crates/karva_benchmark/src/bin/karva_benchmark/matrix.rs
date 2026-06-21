use std::io;

use anyhow::{Context as _, Result};
use serde::Serialize;

use karva_benchmark::CLI_BENCHMARK_PROJECTS;

const FAST_PROJECT_ITERATIONS: usize = 21;
const MEDIUM_PROJECT_ITERATIONS: usize = 15;
const SLOW_PROJECT_ITERATIONS: usize = 9;

#[derive(Debug, Serialize)]
struct Matrix {
    include: Vec<MatrixProject>,
}

#[derive(Debug, Serialize)]
struct MatrixProject {
    project: &'static str,
    iterations: usize,
}

pub fn list_projects() -> Result<()> {
    let matrix = Matrix {
        include: CLI_BENCHMARK_PROJECTS
            .iter()
            .map(|project| MatrixProject {
                project: project.name,
                iterations: matrix_iterations(project.name),
            })
            .collect(),
    };

    serde_json::to_writer(io::stdout(), &matrix).context("Failed to write benchmark matrix")?;
    println!();

    Ok(())
}

fn matrix_iterations(project_name: &str) -> usize {
    match project_name {
        "packaging" | "pyparsing" => SLOW_PROJECT_ITERATIONS,
        "tomlkit" => MEDIUM_PROJECT_ITERATIONS,
        _ => FAST_PROJECT_ITERATIONS,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FAST_PROJECT_ITERATIONS, MEDIUM_PROJECT_ITERATIONS, SLOW_PROJECT_ITERATIONS,
        matrix_iterations,
    };

    #[test]
    fn matrix_iterations_are_higher_for_fast_projects() {
        assert_eq!(matrix_iterations("sniffio"), FAST_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("h11"), FAST_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("tomlkit"), MEDIUM_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("packaging"), SLOW_PROJECT_ITERATIONS);
        assert_eq!(matrix_iterations("pyparsing"), SLOW_PROJECT_ITERATIONS);
    }
}
