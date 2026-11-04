use inviscid::buffer::SourceLine;
use inviscid::markdown::{OutlineItem, collect_outline};

#[test]
fn collect_outline_extracts_headings_and_unique_anchors() {
    let lines = vec![
        "# Project *Overview*".to_string(),
        "## Setup ##".to_string(),
        "```markdown".to_string(),
        "# Ignored fenced heading".to_string(),
        "```".to_string(),
        "> ### Quoted heading".to_string(),
        "## Setup ##".to_string(),
    ];

    assert_eq!(
        collect_outline(&lines),
        vec![
            OutlineItem {
                level: 1,
                title: "Project Overview".to_string(),
                source_line: SourceLine::new(0),
                anchor: "project-overview".to_string(),
            },
            OutlineItem {
                level: 2,
                title: "Setup".to_string(),
                source_line: SourceLine::new(1),
                anchor: "setup".to_string(),
            },
            OutlineItem {
                level: 3,
                title: "Quoted heading".to_string(),
                source_line: SourceLine::new(5),
                anchor: "quoted-heading".to_string(),
            },
            OutlineItem {
                level: 2,
                title: "Setup".to_string(),
                source_line: SourceLine::new(6),
                anchor: "setup-2".to_string(),
            },
        ]
    );
}

#[test]
fn collect_outline_keeps_empty_heading_for_navigation() {
    let lines = vec!["# ".to_string(), "##   ".to_string()];

    assert_eq!(
        collect_outline(&lines),
        vec![
            OutlineItem {
                level: 1,
                title: String::new(),
                source_line: SourceLine::new(0),
                anchor: "heading".to_string(),
            },
            OutlineItem {
                level: 2,
                title: String::new(),
                source_line: SourceLine::new(1),
                anchor: "heading-2".to_string(),
            },
        ]
    );
}
