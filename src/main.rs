use clap::Parser;
use error_chain::error_chain;
use reqwest::StatusCode;
use select::document::Document;
use select::predicate::Name;
use serde::Deserialize;
use std::collections::HashSet;
use url::{Position, Url};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// URL to check
    #[arg(short, long)]
    url: String,

    /// Find broken links in page
    #[arg(short, long = "find-broken-links")]
    links: bool,

    /// Is xml sitemap
    #[arg(short, long = "is-xml-sitemap")]
    is_xml_sitemap: bool,

    /// Find broken images in page
    #[arg(short, long = "find-broken-images")]
    check_images: bool,
}

#[derive(Debug, Deserialize)]
struct UrlSet {
    // #[serde(rename = "url", default)]
    // urls: Vec<UrlEntry>,
}
#[derive(Debug, Deserialize)]
struct UrlEntry {
    // #[serde(rename = "loc")]
    // loc: String,
}
error_chain! {
    foreign_links {
        ReqError(reqwest::Error);
        IoError(std::io::Error);
        UrlParseError(url::ParseError);
        JoinError(tokio::task::JoinError);
    }
}

async fn get_base_url(url: &Url, doc: &Document) -> Result<Url> {
    let base_tag_href = doc.find(Name("base")).filter_map(|n| n.attr("href")).nth(0);
    let base_url =
        base_tag_href.map_or_else(|| Url::parse(&url[..Position::BeforePath]), Url::parse)?;
    Ok(base_url)
}

async fn check_link(url: &Url) -> Result<bool> {
    let res = reqwest::get(url.as_ref()).await?;
    match res.status() {
        StatusCode::OK => Ok(true),
        _ => Ok(false),
    }
}

async fn find_broken_links(links: Vec<String>) -> Result<()> {
    let mut tasks = vec![];

    for link in links {
        let url = Url::parse(&link)?;

        tasks.push(tokio::spawn(async move {
            if !check_link(&url).await.unwrap() {
                println!("{} is Broken", link);
            }
        }));
    }

    for task in tasks {
        task.await?
    }

    Ok(())
}

fn extract_urls(document: &Document) -> Vec<String> {
    let mut urls = Vec::new();

    for node in document.find(Name("loc")) {
        if let Some(url) = Some(node.text()) {
            urls.push(url.to_string());
        }
    }

    urls
}

fn filter_urls(urls: Vec<String>, domain: &Url) -> Vec<String> {
    urls.into_iter()
        .filter(|url| url.contains(domain.host_str().unwrap()))
        .collect()
}

async fn get_document(url: &Url) -> Result<Document> {
    let res = reqwest::get(url.as_ref()).await?.text().await?;
    let document = Document::from(res.as_str());
    Ok(document)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let url = Url::parse(&args.url)?;
    let document = get_document(&url).await?;
    let base_url = get_base_url(&url, &document).await?;

    println!("Starting...");
    if args.is_xml_sitemap {
        let urls = extract_urls(&document);
        let filtered_urls = filter_urls(urls, &base_url);

        if args.links {
            // xml links to each xml page, page=1, page=2, etc
            for url in filtered_urls {
                let internal_document = get_document(&Url::parse(&url)?).await?;
                let internal_urls = extract_urls(&internal_document);
                let internal_filtered_urls = filter_urls(internal_urls, &base_url);

                find_broken_links(internal_filtered_urls).await?;
            }
        }
    } else {
        let base_parser = Url::options().base_url(Some(&base_url));

        let links: HashSet<Url> = document
            .find(Name(if args.check_images { "img" } else { "a" }))
            .filter_map(|n| n.attr("href").or(n.attr("src")))
            .filter_map(|link| base_parser.parse(link).ok())
            .collect();
        let mut tasks = vec![];

        for link in links {
            tasks.push(tokio::spawn(async move {
                if check_link(&link).await.unwrap() {
                    println!("{} is OK", link);
                } else {
                    println!("{} is Broken", link);
                }
            }));
        }

        for task in tasks {
            task.await?
        }
    }

    println!("Done!");

    Ok(())
}
