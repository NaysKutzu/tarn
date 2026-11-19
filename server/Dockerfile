FROM rust:1-slim AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release --locked

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/tarn-jam-server /tarn-jam-server
ENV TARN_JAM_BIND=0.0.0.0:47821
EXPOSE 47821
USER nonroot
ENTRYPOINT ["/tarn-jam-server"]
