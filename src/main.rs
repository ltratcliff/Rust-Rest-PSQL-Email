use axum::{http::StatusCode, routing::post, Json, Router};
use lettre::message::header::ContentType;
use lettre::{Message, SmtpTransport, Transport};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io::Read;
use std::net::SocketAddr;
use std::{env, fs::File};
use tokio_postgres::NoTls;

//TODO - CI/CD for gitlab
//TODO - Readme: howto

#[tokio::main]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();

    // build our application with a route
    let app = Router::new().route("/grafana-acct-request", post(create_user));

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn create_user(Json(payload): Json<FromAngular>) -> (StatusCode, Json<User>) {
    let debug_mode = match env::var("DEBUG") {
        Ok(v) => v,
        Err(_err) => "False".to_string(),
    };
    let email_cc = match env::var("EMAILCC") {
        Ok(v) => v,
        Err(_err) => "SOCOM.CDO.Engineers.DL@socom.mil".to_string(),
    };

    let user = User {
        first_name: payload.firstName,
        last_name: payload.lastName,
        org_name: payload.orgName,
        email_address: payload.email,
    };

    let mut template = File::open("email.tmpl").unwrap();
    let mut email_template = String::new();
    template.read_to_string(&mut email_template).unwrap();
    email_template = email_template.replace("{first}", &user.first_name);
    email_template = email_template.replace("{last}", &user.last_name);

    match db_update(&user).await {
        Ok(()) => tracing::info!("Updating DB"),
        Err(e) => tracing::warn!(e),
    }

    if debug_mode == "True" {
        print!("{}", email_template)
    }

    if debug_mode == "False" {
        let email = Message::builder()
            .from(
                "CDAO Infra Team <SOCOM.CDO.Engineers.DL@socom.mil>"
                    .parse()
                    .unwrap(),
            )
            .to(format!("<{}>", user.email_address).parse().unwrap())
            .cc(format!("<{}>", email_cc).parse().unwrap())
            .subject("CDAO Grafana Account Request")
            .header(ContentType::TEXT_HTML)
            .body(email_template)
            .unwrap();

        // Open a remote connection to gmail
        let mailer = SmtpTransport::relay("localhost").unwrap().build();

        // Send the email
        match mailer.send(&email) {
            Ok(_) => println!("Email sent successfully!"),
            Err(e) => tracing::error!("{}", e),
        }
    }
    // this will be converted into a JSON response
    // with a status code of `201 Created`
    (StatusCode::CREATED, Json(user))
}

async fn db_update(user: &User) -> Result<(), Box<dyn Error>> {
    let puser = match env::var("PSQLUSER") {
        Ok(v) => v,
        Err(_err) => "postgres".to_string(),
    };
    let ppw = match env::var("PSQLPW") {
        Ok(v) => v,
        Err(_err) => "password".to_string(),
    };
    let phost = match env::var("PSQLHOST") {
        Ok(v) => v,
        Err(_err) => "localhost".to_string(),
    };
    let dbname = match env::var("PSQLDBNAME") {
        Ok(v) => v,
        Err(_err) => "rfs".to_string(),
    };
    let cstring = format!(
        "user={} password={} host={} dbname={}",
        puser, ppw, phost, dbname
    );
    let (client, connection) = tokio_postgres::connect(&cstring, NoTls).await?;

    // The connection object performs the actual communication with the database,
    // so spawn it off to run on its own.
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    //Create Table
    tracing::info!("Creating table if not exists");
    client
        .batch_execute(
            "
        CREATE TABLE IF NOT EXISTS grafana (
            id              SERIAL PRIMARY KEY,
            first           VARCHAR NOT NULL,
            last            VARCHAR NOT NULL,
            org             VARCHAR NOT NULL,
            email           VARCHAR NOT NULL,
            date            DATE NOT NULL
        )
    ",
        )
        .await?;

    let pquery = "INSERT INTO grafana (first, last, org, email, date) 
                        VALUES ($1, $2, $3, $4, $5)"
        .to_string();
    let now = chrono::offset::Local::now();
    // let nowstr = now.format("%Y-%m-%d");
    tracing::info!(
        "Creating record: {}, {}, {}, {}",
        &user.first_name,
        &user.last_name,
        &user.org_name,
        &user.email_address
    );
    client
        .execute(
            &pquery,
            &[
                &user.first_name,
                &user.last_name,
                &user.org_name,
                &user.email_address,
                &now.date_naive(),
            ],
        )
        .await?;

    Ok(())
}

// the input to our `create_user` handler
#[allow(non_snake_case)]
#[derive(Deserialize)]
struct FromAngular {
    firstName: String,
    lastName: String,
    orgName: String,
    email: String,
}

// the output to our `create_user` handler
#[derive(Debug, Serialize)]
struct User {
    first_name: String,
    last_name: String,
    org_name: String,
    email_address: String,
}
