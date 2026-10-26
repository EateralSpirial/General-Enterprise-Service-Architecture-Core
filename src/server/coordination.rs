//! Redis coordinates online work; SQLite retains admission across Redis loss.
use crate::runtime;
use anyhow::{ensure, Result};
use std::time::Duration;
fn connection(url: &str) -> Result<redis::Connection> {
    let c = redis::Client::open(url)?.get_connection_with_timeout(Duration::from_secs(3))?;
    c.set_read_timeout(Some(Duration::from_secs(3)))?;
    c.set_write_timeout(Some(Duration::from_secs(3)))?;
    Ok(c)
}
fn key(host: &str, account: &str, device: &str, instance: &str) -> String {
    format!(
        "gesac:{host}:{}",
        runtime::digest(format!("{account}\n{device}\n{instance}").as_bytes())
    )
}
pub fn claim(
    url: &str,
    host: &str,
    account: &str,
    device: &str,
    instance: &str,
    id: &str,
) -> Result<()> {
    let result:i32=redis::Script::new("local v=redis.call('GET',KEYS[1]); if v and v~=ARGV[1] then return 0 end; redis.call('SET',KEYS[1],ARGV[1],'EX',86400);return 1").key(key(host,account,device,instance)).arg(id).invoke(&mut connection(url)?)?;
    ensure!(result == 1, "online target already claimed");
    Ok(())
}
pub fn project(
    url: &str,
    host: &str,
    account: &str,
    device: &str,
    instance: &str,
    id: &str,
    status: &str,
) -> Result<()> {
    let terminal = ["applied", "rolled_back", "cancelled"].contains(&status);
    let _:i32=redis::Script::new("local v=redis.call('GET',KEYS[1]);if v and v~=ARGV[1] then return 0 end;redis.call('SET',KEYS[2],ARGV[2],'EX',2592000);if ARGV[3]=='1' then redis.call('DEL',KEYS[1]) else redis.call('SET',KEYS[1],ARGV[1],'EX',86400) end;return 1")
        .key(key(host,account,device,instance)).key(format!("gesac:{host}:operation:{id}")).arg(id).arg(status).arg(if terminal{"1"}else{"0"}).invoke(&mut connection(url)?)?;
    Ok(())
}
