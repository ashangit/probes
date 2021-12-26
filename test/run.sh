#!/bin/bash

docker rm dev-consul --force
docker rm dev-memcached --force
docker run -d --rm --name=dev-consul --network host -v /home/nfraison/dev/perso/rust/probes/test/conf/consul:/consul/config consul
docker run -d --rm --name=dev-memcached --network host memcached -m 128