FROM redhat/ubi8-minimal:latest as builder

ENV PATH "/root/.cargo/bin:${PATH}"

RUN microdnf install gcc make cmake curl clang openssl-devel -y && \
    curl -sSf https://sh.rustup.rs -o rustinstall.sh && \
    sh rustinstall.sh -q -y && \
    rm -f rustinstall.sh

# ADD LOCAL REPO
ADD . /probes

WORKDIR /probes

RUN make build

# Base image
FROM redhat/ubi8-minimal:latest

# Maintainer
LABEL maintainer="n.fraison <nfraison@yahoo.fr>"

RUN microdnf upgrade -y && \
    microdnf install openssl -y && \
    microdnf clean all

WORKDIR /

COPY --from=builder /probes/target/release/mempoke .

RUN chgrp 0 /mempoke && \
    chmod g=u /mempoke

ENTRYPOINT ["/mempoke"]