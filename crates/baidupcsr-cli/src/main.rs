use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_command(&args) {
        CommandAction::Doctor { config_dir } => {
            print_doctor(config_dir);
            ExitCode::SUCCESS
        }
        CommandAction::TuiShell => {
            let app = build_tui_app_from_config();
            match baidupcsr_tui::run_terminal_app(app) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("tui_error: {err}");
                    ExitCode::from(1)
                }
            }
        }
        CommandAction::UsageError => {
            eprintln!("usage: baidupcsr-cli [tui|doctor [--redact]]");
            ExitCode::from(2)
        }
    }
}

enum CommandAction {
    Doctor { config_dir: Option<PathBuf> },
    TuiShell,
    UsageError,
}

fn parse_command(args: &[String]) -> CommandAction {
    match args {
        [] => CommandAction::TuiShell,
        [cmd] if cmd == "tui" => CommandAction::TuiShell,
        [cmd] if cmd == "doctor" => CommandAction::Doctor { config_dir: None },
        [cmd, flag] if cmd == "doctor" && flag == "--redact" => {
            CommandAction::Doctor { config_dir: None }
        }
        [cmd, flag, dir_flag, dir]
            if cmd == "doctor" && flag == "--redact" && dir_flag == "--config-dir" =>
        {
            CommandAction::Doctor {
                config_dir: Some(PathBuf::from(dir)),
            }
        }
        _ => CommandAction::UsageError,
    }
}

fn build_tui_app_from_config() -> baidupcsr_tui::TuiApp {
    let discovery = baidupcsr_config::discovery_input_from_env(None);
    let config_path = baidupcsr_config::discover_config_file(&discovery);
    let config = match baidupcsr_config::load_config(&config_path) {
        Ok(config) => config,
        Err(err) => {
            let mut app = baidupcsr_tui::TuiApp::new("/");
            app.set_notice(format!("config_error: {err}"));
            return app;
        }
    };

    let Some(user) = baidupcsr_config::active_user(&config) else {
        let mut app = baidupcsr_tui::TuiApp::new("/");
        app.set_notice("active_user: missing");
        return app;
    };

    let workdir = if user.workdir.is_empty() {
        "/"
    } else {
        user.workdir.as_str()
    };
    let mut app = baidupcsr_tui::TuiApp::new(workdir);

    match load_remote_entries(&config, user, workdir) {
        Ok(entries) => {
            app.set_entries(entries);
            app.set_notice("remote list loaded from BaiduPCS-Go config");
        }
        Err(message) => {
            app.set_notice(message);
        }
    }

    app
}

fn load_remote_entries(
    config: &baidupcsr_config::PcsConfig,
    user: &baidupcsr_config::BaiduUser,
    workdir: &str,
) -> Result<Vec<baidupcsr_pcs::FileEntry>, String> {
    let auth = if !user.cookies.is_empty() {
        Some(baidupcsr_pcs::PcsAuth::from_cookie_header(
            user.cookies.as_str(),
        ))
    } else {
        baidupcsr_pcs::PcsAuth::from_bduss_stoken(
            user.bduss.as_str(),
            Some(user.stoken.as_str()),
            Some(user.baiduid.as_str()),
        )
    }
    .ok_or_else(|| "auth_error: missing BDUSS/cookies in active user".to_string())?;

    let pcs_addr = if config.pcs_addr.is_empty() {
        "pcs.baidu.com"
    } else {
        config.pcs_addr.as_str()
    };
    let user_agent_values = [
        config.pcs_ua.as_str(),
        config.user_agent.as_str(),
        config.pan_ua.as_str(),
    ];
    let user_agent = first_non_empty(&user_agent_values).unwrap_or("netdisk");

    let api = baidupcsr_pcs::PcsApi::new(baidupcsr_pcs::PcsClientOptions {
        app_id: config.appid,
        pcs_addr: pcs_addr.to_string(),
        enable_https: config.enable_https,
    });
    let client = baidupcsr_pcs::BlockingPcsClient::new(
        api,
        auth,
        baidupcsr_pcs::BlockingPcsClientOptions::new(
            user_agent,
            optional_string(config.proxy.as_str()),
        ),
    );

    client
        .list(
            workdir,
            baidupcsr_pcs::OrderOptions {
                by: baidupcsr_pcs::OrderBy::Name,
                direction: baidupcsr_pcs::OrderDirection::Asc,
            },
        )
        .map_err(|err| format!("remote_list_error: {err}"))
}

fn first_non_empty<'a>(values: &'a [&str]) -> Option<&'a str> {
    values.iter().copied().find(|value| !value.is_empty())
}

fn optional_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn print_doctor(config_dir: Option<PathBuf>) {
    println!("baidupcsr doctor");
    println!("redacted: true");
    let discovery = baidupcsr_config::discovery_input_from_env(config_dir);
    let config_path = baidupcsr_config::discover_config_file(&discovery);
    println!("config_path: {}", config_path.display());

    match baidupcsr_config::load_config(&config_path) {
        Ok(config) => print_loaded_config(&config),
        Err(err) => {
            println!("config: not loaded");
            println!("config_error: {err}");
        }
    }

    println!("crates:");
    println!("  - {}", baidupcsr_config::crate_name());
    println!("  - {}", baidupcsr_core::crate_name());
    println!("  - {}", baidupcsr_pcs::crate_name());
    println!("  - {}", baidupcsr_preview::crate_name());
    println!("  - {}", baidupcsr_tui::crate_name());
}

fn print_loaded_config(config: &baidupcsr_config::PcsConfig) {
    println!("config: loaded");
    println!("active_uid: {}", config.baidu_active_uid);
    println!("accounts: {}", config.baidu_user_list.len());

    match baidupcsr_config::active_user(config) {
        Some(user) => {
            let auth = user.auth_presence();
            println!("workdir: {}", user.workdir);
            println!("bduss: {}", presence(auth.bduss));
            println!("stoken: {}", presence(auth.stoken));
            println!("cookies: {}", presence(auth.cookies));
            println!("access_token: {}", presence(auth.access_token));
        }
        None => {
            println!("active_user: missing");
        }
    }
}

fn presence(value: bool) -> &'static str {
    if value {
        "present"
    } else {
        "missing"
    }
}
