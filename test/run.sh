#!/bin/bash

SCRIPT_DIR="$(dirname `realpath "$0"`)"

docker rm dev-consul --force
docker rm dev-memcached --force
docker rm dev-memcached2 --force
docker rm dev-memcached3 --force
docker rm dev-memcached4 --force
docker rm dev-grafana --force
docker rm dev-prometheus --force
docker run -d --rm --pull always --name=dev-consul --network host -v ${SCRIPT_DIR}/conf/consul:/consul/config consul
docker run -d --rm --pull always --name=dev-memcached --network host memcached -m 128
docker run -d --rm --pull always --name=dev-memcached2 --network host memcached -m 128 -p 11212
docker run -d --rm --pull always --name=dev-memcached3 --network host memcached -m 128 -p 11213
docker run -d --rm --pull always --name=dev-memcached4 --network host memcached -m 128 -p 11214
docker run -d --rm --pull always --name=dev-grafana --network host grafana/grafana:latest
docker run -d --rm --pull always --name=dev-prometheus --network host -v ${SCRIPT_DIR}/conf/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml prom/prometheus:latest
