use std::process::Command;
use std::fs;
use similar::{ChangeTag, TextDiff};

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
            "rs,md",
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

    if output_content.trim() != ground_truth.trim() {
        // Create a diff of the two strings
        let diff = TextDiff::from_lines(
            ground_truth.trim(),
            output_content.trim()
        );

        // Build detailed error message
        let mut error_msg = String::from("\nDifferences found between output and ground truth:\n");
        
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "- ",
                ChangeTag::Insert => "+ ",
                ChangeTag::Equal => "  ",
            };
            error_msg.push_str(&format!("{}{}", sign, change));
        }

        panic!("{}", error_msg);
    }
}


#[test]
fn test_dirscribe_diff_only_output_matches_ground_truth() {
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
            "rs,md",
            "--exclude-paths=tests",
            "--output-path=tests/output/dirscribe-output.txt",
            "--diff-only",
            "--start-commit-id=1420e8e8126bab612a55f45c40ece45fa338958",
            "--end-commit-id=d7f174db9aa03359d33b7a8e1b18944b88b74f35"

        ])
        .output()
        .expect("Failed to run dirscribe");
    assert!(output.status.success(), "dirscribe command failed");

    // Read and compare output with ground truth
    let output_content = fs::read_to_string("tests/output/dirscribe-output.txt")
        .expect("Failed to read output file");
    let ground_truth = fs::read_to_string("tests/data/ground-truth-diff.txt")
        .expect("Failed to read ground truth file");

    if output_content.trim() != ground_truth.trim() {
        // Create a diff of the two strings
        let diff = TextDiff::from_lines(
            ground_truth.trim(),
            output_content.trim()
        );

        // Build detailed error message
        let mut error_msg = String::from("\nDifferences found between output and ground truth:\n");
        
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "- ",
                ChangeTag::Insert => "+ ",
                ChangeTag::Equal => "  ",
            };
            error_msg.push_str(&format!("{}{}", sign, change));
        }

        panic!("{}", error_msg);
    }
}
