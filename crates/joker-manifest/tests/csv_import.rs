//! `jk profile import csv` (ref/03 §2.2.19).

use joker_manifest::csv_import::import_csv;
use joker_manifest::source::Sources;

fn ids(diags: &[&joker_manifest::diag::Diagnostic]) -> Vec<&'static str> {
    diags.iter().map(|d| d.code).collect()
}

#[test]
fn the_bench_esp32_map_imports_to_the_documented_draft() {
    let csv = "signal,fc,at,type,direction,unit,scale,offset,label,description\n\
led,5,0,bool,output,,,,LED,board LED (GPIO2)\n\
led_rb,1,0,bool,,,,,,\n\
button,2,0,bool,,,,,BOOT,\"BOOT button, already inverted: 1 = pressed\"\n\
touch,4,0,u16,,,,,,raw value of touch sensor T0 (GPIO4); drops when touched\n\
rssi,4,1,i16,,,,,,Wi-Fi RSSI in dBm; 0 without connection\n\
uptime,4,2,u32,,s,,,,seconds since boot\n\
fw,3,0,u16,param,,,,,bench firmware version\n";
    let mut sources = Sources::default();
    let file = sources.add("map.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/bench-esp32", "modbus", false);
    assert!(result.diagnostics.sorted(&sources).is_empty());
    let toml = result.toml.expect("a draft");
    assert!(toml.contains("schema = 1"));
    assert!(toml.contains(r#"name = "local/bench-esp32""#) || toml.contains("local/bench-esp32"));
    assert!(
        toml.contains(r#""led"    = { at = 0, fc = 5"#)
            || toml.contains(r#""led" = { at = 0, fc = 5"#)
    );
    assert!(toml.contains("direction = \"output\""));
    assert!(toml.contains("unit = \"s\""));
    assert!(toml.contains("# REVIEW"));
    assert!(toml.contains("poll = \"1s\""));
}

#[test]
fn without_a_header_or_a_required_column_is_j0803() {
    let csv = "not,a,register,map\n1,2,3,4\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0803"]);
    assert!(result.toml.is_none());
}

#[test]
fn an_unknown_column_is_j0140() {
    let csv = "signal,fc,at,bogus\nx,3,0,1\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0140"]);
}

#[test]
fn a_repeated_column_is_j0140() {
    let csv = "signal,fc,at,fc\nx,3,0,3\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0140"]);
}

#[test]
fn an_invalid_fc_is_j0810() {
    let csv = "signal,fc,at\nx,9,0\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0810"]);
}

#[test]
fn an_address_out_of_range_is_j0810() {
    let csv = "signal,fc,at\nx,3,70000\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0810"]);
}

#[test]
fn a_type_incompatible_with_the_fc_is_j0809() {
    let csv = "signal,fc,at,type\nx,5,0,u16\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0809"]);
}

#[test]
fn a_direction_incompatible_with_the_fc_is_j0810() {
    let csv = "signal,fc,at,direction\nx,3,0,output\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0810"]);
}

#[test]
fn a_unit_on_a_bool_signal_is_j0815() {
    let csv = "signal,fc,at,type,unit\nx,1,0,bool,s\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0815"]);
}

#[test]
fn an_invalid_unit_is_j0815() {
    let csv = "signal,fc,at,type,unit\nx,3,0,u16,bogus-unit\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0815"]);
}

#[test]
fn a_zero_scale_is_j0806() {
    let csv = "signal,fc,at,type,scale\nx,3,0,u16,0\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0806"]);
}

#[test]
fn a_comma_decimal_is_j0806_with_help() {
    let csv = "signal;fc;at;type;scale\nx;3;0;u16;0,1\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    let diags = result.diagnostics.sorted(&sources);
    assert_eq!(ids(&diags), ["J0806"]);
    assert!(diags[0].help.iter().any(|h| h.contains("0.1")));
}

#[test]
fn a_repeated_signal_name_is_j0811() {
    let csv = "signal,fc,at\nx,3,0\nx,3,1\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0811"]);
}

#[test]
fn an_address_collision_in_the_same_register_space_is_j0811() {
    let csv = "signal,fc,at\nx,3,0\ny,3,0\n";
    let mut sources = Sources::default();
    let file = sources.add("bad.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0811"]);
}

#[test]
fn comments_and_blank_lines_are_ignored() {
    let csv = "# a datasheet export\nsignal,fc,at\n\nx,3,0\n# trailing note\n";
    let mut sources = Sources::default();
    let file = sources.add("ok.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert!(result.diagnostics.sorted(&sources).is_empty());
    assert!(result.toml.is_some());
}

#[test]
fn a_bom_is_accepted_and_ignored() {
    let csv = "\u{feff}signal,fc,at\nx,3,0\n";
    let mut sources = Sources::default();
    let file = sources.add("ok.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", false);
    assert!(result.diagnostics.sorted(&sources).is_empty());
}

#[test]
fn modicon_notation_converts_and_checks_the_space() {
    let csv = "signal,fc,at\nx,3,40001\n";
    let mut sources = Sources::default();
    let file = sources.add("ok.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "modbus", true);
    assert!(result.diagnostics.sorted(&sources).is_empty());
    assert!(result.toml.unwrap().contains("at = 0"));

    let csv2 = "signal,fc,at\nx,3,30001\n"; // wrong space for fc 3 (holding = 4xxxx)
    let mut sources2 = Sources::default();
    let file2 = sources2.add("bad.csv".into(), csv2.to_owned());
    let result2 = import_csv(file2, csv2, "local/x", "modbus", true);
    assert_eq!(ids(&result2.diagnostics.sorted(&sources2)), ["J0810"]);
}

#[test]
fn a_driver_other_than_modbus_is_a_usage_error() {
    let csv = "signal,fc,at\nx,3,0\n";
    let mut sources = Sources::default();
    let file = sources.add("ok.csv".into(), csv.to_owned());
    let result = import_csv(file, csv, "local/x", "opcua", false);
    assert_eq!(ids(&result.diagnostics.sorted(&sources)), ["J0808"]);
}
