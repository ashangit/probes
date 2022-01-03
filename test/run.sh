#!/bin/bash

docker rm dev-consul --force
docker rm dev-memcached --force
docker rm dev-grafana --force
docker rm dev-prometheus --force
docker run -d --rm --name=dev-consul --network host -v /home/nfraison/dev/perso/rust/probes/test/conf/consul:/consul/config consul
docker run -d --rm --name=dev-memcached --network host memcached -m 128
docker run -d --rm --name=dev-grafana --network host grafana/grafana:latest
docker run -d --rm --name=dev-prometheus --network host -v /home/nfraison/dev/perso/rust/probes/test/conf/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml prom/prometheus:latest
