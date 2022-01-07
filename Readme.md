#TODO
- use rate instead of sleep interval relying on token bucket?
- manage stop probes on nodes + remove old metrics
- do some bench + add some integration tests
- using multiplexing to send multiple request on the same host even if no response from previous?
- manage reconnection on memcached client if network failure: use multi channel between disco and clients to indicate which client is dead and relaunch it
- check unittest
- what else?