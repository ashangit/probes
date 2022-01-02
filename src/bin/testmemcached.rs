//use console_subscriber;
use log::error;

use probes::memcached;

fn main() -> Result<(), i32> {
    let mt_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("MemPoke")
        .build();

    match mt_rt {
        Err(issue) => {
            error!(
                "Issue running the multi thread event loop due to: {}",
                issue
            );
            return Err(0);
        }
        Ok(mt) => mt.block_on(async {
            let mut c_memcache = memcached::connect("localhost:11211").await.unwrap();
            c_memcache.set("nico", "value2".as_bytes().to_vec()).await;
        }),
    }

    Ok(())
}
