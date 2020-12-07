FROM rust:latest AS build-stage

RUN mkdir -p /usr/src/
COPY . /usr/src/
WORKDIR /usr/src
RUN cargo clean && cargo build --release

FROM debian:latest

RUN apt-get update && apt-get install -y openssh-client ffmpeg rsync
RUN useradd -m user
RUN mkdir -p /home/user/.ssh
COPY id_rsa_shared /home/user/.ssh/id_rsa
RUN chown -R user:user /home/user/.ssh

COPY --from=build-stage /usr/src /home/user/app
RUN chown -R user:user /home/user/app

USER user
WORKDIR /home/user/app

CMD ["/bin/bash"]
