docker rm dev-consul --force
docker rm dev-memcached --force
docker run -d --name=dev-consul --network host consul
docker run -d --rm --name=dev-memcached --network host memcached -m 128