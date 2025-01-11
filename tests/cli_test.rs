use std::process::Command;
use std::fs;

#[test]
fn test_dirscribe_output_matches_ground_truth() {
    // Install dirscribe from the current directory
    let output = Command::new("cargo")
        .args(["install", "--path", "."])
        .output()
        .expect("Failed to install dirscribe");
    assert!(output.status.success(), "Failed to install dirscribe");

    // Create temporary output directory if it doesn't exist
    fs::create_dir_all("tests/output").expect("Failed to create output directory");

    // Run dirscribe command
    let output = Command::new("dirscribe")
        .args([
            ".",
            "rs,md",
            "--use-gitignore",
            "--exclude-paths=tests",
            "--output-path=tests/output/dirscribe-output.txt"
        ])
        .output()
        .expect("Failed to run dirscribe");
    assert!(output.status.success(), "dirscribe command failed");

    // Read and compare output with ground truth
    let output_content = fs::read_to_string("tests/output/dirscribe-output.txt")
        .expect("Failed to read output file");
    let ground_truth = fs::read_to_string("tests/data/ground-truth.txt")
        .expect("Failed to read ground truth file");

    assert_eq!(
        output_content.trim(),
        ground_truth.trim(),
        "Output does not match ground truth"
    );
}
