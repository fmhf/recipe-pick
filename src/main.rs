use clap::Parser;
use colored::Colorize;
use csv::ReaderBuilder;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use ulid::Ulid;

const AUTH_ENDPOINT: &str = "https://auth-service.live-k8s.hellofresh.io";
const CPS_ENDPOINT: &str = "https://culinary-planning-service.live-k8s.hellofresh.io";

#[derive(Debug, Deserialize)]
struct Config {
    username: String,
    password: String,
    key: String,
    secret: String,
    country: String,
}

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
struct Cli {
    #[clap(short, long, help = "csv file path")]
    file: String,
    #[clap(short, long, help = "market", default_value = "it")]
    market: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Err(err) = get_recipe_picklist(&cli).await {
        eprintln!("{} {}", "Error:".red(), err);
        std::process::exit(1);
    };

    Ok(())
}

async fn get_recipe_picklist(cli: &Cli) -> anyhow::Result<()> {
    let config: Config = serde_yaml::from_slice(&std::fs::read("config.yaml")?)?;
    let mut rdr = ReaderBuilder::new().delimiter(b',').from_path(&cli.file)?;

    let mut codes: Vec<String> = vec![];
    for result in rdr.records() {
        let record = result?;
        codes.push(record.get(0).unwrap().to_string());
    }

    if codes.is_empty() {
        anyhow::bail!("No codes found in csv file");
    }

    let client = reqwest::Client::new();
    let token = get_token(&client, &config).await?;

    get_picklists(&client, &token, &cli.market, &codes).await?;

    Ok(())
}

#[derive(Debug, Deserialize)]
struct Token {
    access_token: String,
}

async fn get_token(client: &reqwest::Client, config: &Config) -> anyhow::Result<String> {
    let resp = client
        .post(format!("{}/token", AUTH_ENDPOINT))
        .query(&[
            ("grant_type", "password"),
            ("username", &config.username),
            ("password", &config.password),
            ("country", &config.country),
        ])
        .basic_auth(&config.key, Some(&config.secret))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!(resp.text().await?);
    }

    let token = resp.json::<Token>().await?;

    Ok(token.access_token)
}

#[derive(Debug, Deserialize)]
struct RecipeResult {
    recipes: Vec<Recipe>,
}

#[derive(Debug, Deserialize)]
struct Recipe {
    title: String,
    #[serde(rename = "cskus")]
    skus: Vec<Sku>,
}

#[derive(Debug, Deserialize)]
struct Sku {
    code: String,
    name: String,
    servings_ratio: HashMap<String, f64>,
}

impl Sku {
    fn picklist(&self, servings: u32) -> String {
        let ratio = match self.servings_ratio.get(&servings.to_string()) {
            Some(ratio) => *ratio,
            None => 0.0,
        };

        (ratio.ceil() as u32).to_string()
    }
}

async fn get_recipes(
    client: &reqwest::Client,
    token: &str,
    market: &str,
    codes: &[String],
) -> anyhow::Result<Vec<Recipe>> {
    let resp = client
        .post(format!("{}/{}/recipe/search", CPS_ENDPOINT, market))
        .query(&[("expand", "skus")])
        .header("Authorization", format!("Bearer {}", token))
        .json(&json!({ "codes": codes }))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!(resp.text().await?);
    }

    let recipe_result = resp.json::<RecipeResult>().await?;
    Ok(recipe_result.recipes)
}

async fn get_picklists(
    client: &reqwest::Client,
    token: &str,
    market: &str,
    codes: &[String],
) -> anyhow::Result<()> {
    println!("Getting recipes...");
    let recipes = get_recipes(client, token, market, codes).await?;

    if recipes.is_empty() {
        anyhow::bail!("No recipes found");
    }

    generate_picklist(&recipes)
}

fn generate_picklist(recipes: &[Recipe]) -> anyhow::Result<()> {
    let pb = indicatif::ProgressBar::new(recipes.len() as u64);
    pb.println("Generating picklist...");

    let file_name = format!("{}_picklists.csv", &Ulid::new().to_string());
    let mut wtr = csv::Writer::from_path(&file_name)?;
    wtr.write_record([
        "name",
        "skus.mapping.value",
        "skus.mapping.name",
        "skus.mapping.picks.1",
        "skus.mapping.picks.2",
        "skus.mapping.picks.3",
        "skus.mapping.picks.4",
        "skus.mapping.picks.5",
        "skus.mapping.picks.6",
    ])?;
    for rec in recipes {
        let title = &rec.title;
        for sku in rec.skus.iter() {
            wtr.write_record([
                title,
                &sku.code,
                &sku.name,
                &sku.picklist(1),
                &sku.picklist(2),
                &sku.picklist(3),
                &sku.picklist(4),
                &sku.picklist(5),
                &sku.picklist(6),
            ])?;
        }

        pb.inc(1);
    }
    wtr.flush()?;

    pb.finish();
    println!("Picklist generated: {}", file_name.green());

    Ok(())
}
