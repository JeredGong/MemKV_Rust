use clap::{App, AppSettings, Arg, SubCommand};
use kvs::KvStore;
use std::env::current_dir;
use std::io;
use std::process::exit;

fn main() {
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .setting(AppSettings::DisableHelpSubcommand)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::VersionlessSubcommands)
        .subcommand(
            SubCommand::with_name("set")
                .about("Set the value of a string key to a string")
                .arg(Arg::with_name("KEY").help("A string key").required(true))
                .arg(
                    Arg::with_name("VALUE")
                        .help("The string value of the key")
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("get")
                .about("Get the string value of a given string key")
                .arg(Arg::with_name("KEY").help("A string key").required(true)),
        )
        .subcommand(
            SubCommand::with_name("rm")
                .about("Remove a given key")
                .arg(Arg::with_name("KEY").help("A string key").required(true)),
        )
        .get_matches();

    // 统一处理：匹配子命令
    match matches.subcommand() {
        ("set", Some(matches)) => {
            let key = matches.value_of("KEY").unwrap();
            let value = matches.value_of("VALUE").unwrap();

            let store = KvStore::open(current_dir().unwrap()).unwrap();
            
            if let Err(e) = store.set(key.to_string(), value.to_string()) {
                eprintln!("{}", e);
                exit(1);
            }
        }
        ("get", Some(matches)) => {
            let key = matches.value_of("KEY").unwrap();
            
            // 打开数据库
            let store = KvStore::open(current_dir().unwrap()).unwrap();
            
            // 根据 get 的返回值决定打印什么
            match store.get(key.to_string()) {
                Ok(Some(v)) => println!("{}", v), // 找到值
                Ok(None) => println!("Key not found"), // 未找到
                Err(e) => {
                    eprintln!("{}", e);
                    exit(1);
                }
            }
        }
        ("rm", Some(matches)) => {
            let key = matches.value_of("KEY").unwrap();
            
            let store = KvStore::open(current_dir().unwrap()).unwrap();
            
            match store.remove(key.to_string()) {
                Ok(()) => {}, 
                Err(e) => {
                    if e.kind() == io::ErrorKind::NotFound {
                        println!("Key not found");
                    } else {
                        eprintln!("{}", e);
                    }
                    exit(1);
                }
            }
        }
        _ => unreachable!(),
    }
}
