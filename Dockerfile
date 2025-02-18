FROM alpine AS runner

COPY ./server /bin/server

ENTRYPOINT ["/bin/server"]
