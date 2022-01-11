# TODO
- do some bench + add some integration tests
- using multiplexing to send multiple request on the same host even if no response from previous?
- check unittest
- watch on consul is only done on services which can lead to long time wait if update is only on a node of a service as the service watch is not affected by this change
- see how to refacto match stop_probe_resp_rx.try_recv()
- what else?