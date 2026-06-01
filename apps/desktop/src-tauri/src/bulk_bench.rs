#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkBenchmarkRound {
    pub label: String,
    pub urls: Vec<String>,
}

pub fn parse_bulk_benchmark_rounds(value: &str) -> Result<Vec<BulkBenchmarkRound>, String> {
    let mut rounds = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_urls = Vec::new();

    for raw_line in value.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            push_bulk_benchmark_round(&mut rounds, current_label.take(), &mut current_urls)?;
            continue;
        }

        if let Some((label, remainder)) = line.split_once('=') {
            push_bulk_benchmark_round(&mut rounds, current_label.take(), &mut current_urls)?;
            let label = label.trim();
            if label.is_empty() {
                return Err("Bulk benchmark round labels cannot be empty.".into());
            }
            current_label = Some(label.to_string());
            current_urls.extend(split_bulk_benchmark_urls(remainder));
        } else {
            current_urls.extend(split_bulk_benchmark_urls(line));
        }
    }

    push_bulk_benchmark_round(&mut rounds, current_label.take(), &mut current_urls)?;
    if rounds.is_empty() {
        return Err("Set SDM_BULK_BENCH_URLS to at least one labeled bulk URL round.".into());
    }

    Ok(rounds)
}

fn push_bulk_benchmark_round(
    rounds: &mut Vec<BulkBenchmarkRound>,
    label: Option<String>,
    urls: &mut Vec<String>,
) -> Result<(), String> {
    if label.is_none() && urls.is_empty() {
        return Ok(());
    }
    let label = label.ok_or_else(|| {
        "Bulk benchmark URLs must start with a labeled round, for example hoster=https://..."
            .to_string()
    })?;
    if urls.len() < 2 {
        return Err(format!(
            "Bulk benchmark round {label} needs at least two URLs."
        ));
    }

    rounds.push(BulkBenchmarkRound {
        label,
        urls: std::mem::take(urls),
    });
    Ok(())
}

fn split_bulk_benchmark_urls(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split([',', ';'])
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_benchmark_env_parser_splits_two_hoster_rounds() {
        let rounds = parse_bulk_benchmark_rounds(
            "fuckingfast=https://example.test/a.rar\nhttps://example.test/b.rar\n\n\
             datanodes=https://data.example.test/a.rar, https://data.example.test/b.rar",
        )
        .expect("valid rounds should parse");

        assert_eq!(rounds.len(), 2);
        assert_eq!(rounds[0].label, "fuckingfast");
        assert_eq!(
            rounds[0].urls,
            vec!["https://example.test/a.rar", "https://example.test/b.rar"]
        );
        assert_eq!(rounds[1].label, "datanodes");
        assert_eq!(
            rounds[1].urls,
            vec![
                "https://data.example.test/a.rar",
                "https://data.example.test/b.rar"
            ]
        );
    }

    #[test]
    fn bulk_benchmark_env_parser_rejects_single_link_rounds() {
        let error = parse_bulk_benchmark_rounds("single=https://example.test/a.rar")
            .expect_err("bulk rounds need at least two links");

        assert!(error.contains("at least two"));
    }
}
