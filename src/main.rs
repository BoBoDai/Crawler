use std::collections::HashSet;
use std::sync::{Arc};
use std::time::Duration;
use reqwest::{Client, Url};
use scraper::{Html, Selector};
use tokio::sync::{mpsc, Mutex};

struct Crawler {
    client: Client,
    visited: Arc<Mutex<HashSet<String>>>,
    base_domain: String,
}

impl Crawler {
    pub fn new(base_url: &str) -> Self {
        let base_url = Url::parse(base_url).expect("Invalid base URL");
        let base_domain = base_url.domain().unwrap_or("").to_string();

        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("AsyncCrawler/1.0")
                .build()
                .expect("Failed to create client"),
            visited: Arc::new(Mutex::new(HashSet::new())),
            base_domain,
        }
    }

    async fn fetch(&self, url: String) -> Option<String> {
        match self.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    Some(response.text().await.unwrap_or_default())
                } else {
                    eprintln!("Request failed for {}:{}", url, response.status());
                    None
                }
            }
            Err(e) => {
                eprintln!("Request failed for {}:{}", url, e);
                None
            }
        }
    }

    fn extract_links(&self, html: &str, current_url: &str) -> Vec<String> {
        let document = Html::parse_document(html);
        let title = Selector::parse("title")
            .ok()
            .and_then(|sel| document.select(&sel).next())
            .map(|e| e.text().collect::<Vec<_>>().join(""))
            .unwrap_or_else(|| "(no title)".to_string());

        let selector = Selector::parse("a[href]").unwrap();

        let links: Vec<String> = document
            .select(&selector)
            .filter_map(|element| {
                let href = element.attr("href")?;
                let base = Url::parse(current_url).ok()?;
                let resolved = base.join(href).ok()?;

                if resolved.domain() != Some(&self.base_domain) {
                    return None;
                }

                Some(resolved.to_string())
            }).collect();

        // === 格式化输出 ===
        println!("\n==============================");
        println!("[✓] Crawled: {}", current_url);
        println!("Title   : {}", title);
        if !links.is_empty() {
            println!("\nLinks:");
            for link in &links {
                println!("  - {}", link);
            }
        } else {
            println!("No links found.");
        }
        println!("==============================\n");

        links
    }

    async fn process_url(&self, url: String, tx: mpsc::Sender<String>) {
        if self.is_visited(&url).await {
            return;
        }

        println!("Crawling: {}", url);

        if let Some(html) = self.fetch(url.clone()).await {
            let _ = self.mark_visited(&url).await;

            for link in self.extract_links(&html, &url) {
                if !self.is_visited(&link).await {
                    tx.send(link).await.expect("Failed to send link");
                }
            }
        }
    }
    async fn is_visited(&self, url: &str) -> bool {
        self.visited.lock().await.contains(url)
    }

    async fn mark_visited(&self, url: &str) -> bool {
        self.visited.lock().await.insert(url.to_string())
    }
}

#[tokio::main]
async fn main() {
    let base_url = "https://www.bilibili.com";
    let crawler = Arc::new(Crawler::new(base_url));

    let (tx, rx) = mpsc::channel::<String>(100);
    let rx = Arc::new(Mutex::new(rx));
    let tx_initial = tx.clone();

    tx_initial.send(base_url.to_string()).await.expect("Failed to send initial URL");

    let workers = 5;

    for _ in 0..workers {
        let crawler = Arc::clone(&crawler);
        let tx = tx.clone();
        let rx = Arc::clone(&rx);

        tokio::spawn(async move {
            loop {
                let maybe_url = {
                    let mut rx_locked = rx.lock().await;
                    rx_locked.recv().await
                };
                match maybe_url {
                    None => break,
                    Some(url) => crawler.process_url(url, tx.clone()).await,
                }
            }
        });
    }
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
