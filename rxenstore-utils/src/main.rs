#[macro_use]
extern crate clap;
extern crate libxenstore;
extern crate tokio_core;

use clap::{App, AppSettings, ArgMatches, SubCommand};
use libxenstore::client::Client;
use tokio_core::reactor::Core;


fn main() {
    let app_m = App::new("rxenstore")
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::GlobalVersion)
        .setting(AppSettings::SubcommandRequired)
        .global_setting(AppSettings::ColoredHelp)
        .max_term_width(72)
        .subcommand(SubCommand::with_name("ls").about("lists available entries"))
        .get_matches();

    let mut core = Core::new().unwrap();

    let cl = Client::connect("/dev/xen/xenbus", &core.handle()).unwrap();

    match app_m.subcommand() {
        ("ls", Some(cmd_m)) => ls_cmd(cl, cmd_m),
        _ => unreachable!(),
    }
}

fn ls_cmd(_: Client, _: &ArgMatches) {
    println!("ls cmd")
}
