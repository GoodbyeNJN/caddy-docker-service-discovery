# Caddy Docker Service Discovery

This project provides a Docker service discovery mechanism for Caddy.

## Usage

1. On machine A, start with the following compose file:

```yaml
services:
    dns:
        image: ghcr.io/goodbyenjn/caddy-docker-service-discovery
        container_name: dns
        ports:
            - 3000:3000
            - 5353:53/udp
        environment:
            # (Optional) DNS server listen address and port, default is `0.0.0.0:53`
            # - DNS_SERVER_LISTEN=0.0.0.0:53

            # (Optional) Service registry listen address and port, default is `0.0.0.0:3000`
            # - SERVICE_REGISTRY_LISTEN=0.0.0.0:3000

            # (Required) Hostname for this service registry, used to identify itself
            - SELF_HOSTNAME=alice.com

            # (Optional) URL for other service registries, separated by space
            - REGISTRY_URLS=http://bob.com:3000 http://charlie.com:3000

            # (Optional) Log level, default is `info`
            # - LOG_LEVEL=debug
        volumes:
            # (Required) Docker socket must be mounted at `/var/run/docker.sock`
            - /var/run/docker.sock:/var/run/docker.sock

    whoami:
        image: traefik/whoami
        container_name: whoami
        labels:
            # Register this service to the service registry

            # Ends with `.public` to make it public, will be registered to all service registries
            caddy_0: access-alice-from-everyone.public

            # Ends with `.private` to make it private, will only be registered to this service registry
            caddy_1: only-access-alice-from-alice.private

            # Or just single label
            # caddy: hello-world.public
```

2. On machine B, start with the following compose file:

```yaml
services:
    dns:
        image: ghcr.io/goodbyenjn/caddy-docker-service-discovery
        container_name: dns
        ports:
            - 3000:3000
            - 5353:53/udp
        environment:
            - SELF_HOSTNAME=bob.com
            - REGISTRY_URLS=http://alice.com:3000 http://charlie.com:3000
        volumes:
            - /var/run/docker.sock:/var/run/docker.sock

    whoami:
        image: traefik/whoami
        container_name: whoami
        labels:
            caddy_0: access-bob-from-everyone.public
            caddy_1: only-access-bob-from-bob.private
```

3. Try to resolve the services:

```bash
# Access public service from both service registries
dig +noall +answer @alice.com -p 5353 access-alice-from-everyone access-bob-from-everyone
dig +noall +answer @bob.com -p 5353 access-alice-from-everyone access-bob-from-everyone

# Access private service from its own service registry
dig +noall +answer @alice.com -p 5353 only-access-alice-from-alice
dig +noall +answer @bob.com -p 5353 only-access-bob-from-bob
```

## Integration with Caddy

Recommended to use [caddy-docker-proxy](https://github.com/lucaslorentz/caddy-docker-proxy).

```yaml
# docker-compose.yml

services:
    caddy:
        image: lucaslorentz/caddy-docker-proxy
        container_name: caddy
        restart: unless-stopped
        depends_on:
            - dns
        ports:
            - 80:80
            - 443:443
        networks:
            - caddy
        configs:
            - source: config
              target: /config/caddy/Caddyfile

    dns:
        image: goodbyenjn/caddy-docker-service-discovery
        container_name: dns
        ports:
            - 3000:3000
        networks:
            - caddy
        environment:
            - SELF_HOSTNAME=alice.com
            - REGISTRY_URLS=http://bob.com:3000 http://charlie.com:3000
        volumes:
            - /var/run/docker.sock:/var/run/docker.sock

    whoami:
        image: traefik/whoami
        container_name: whoami
        networks:
            - caddy
        labels:
            caddy_0: for-everyone.public
            caddy_1: only-for-this-machine.private

networks:
    caddy:
        name: caddy
        driver: bridge
        attachable: true

configs:
    config:
        file: ./Caddyfile
```

```caddyfile
# Caddyfile

404.localhost:80 {
	respond "404 Not Found"
}

*.localhost:80 {
	reverse_proxy {
		to "{labels.1}.public:443" "{labels.1}.private:443"

		header_up Host "{http.reverse_proxy.upstream.host}"

		lb_policy first
		lb_retries 2

		transport http {
			resolvers dns
			tls_insecure_skip_verify
		}
	}
}

*.alice.com {
	reverse_proxy {
		header_up Host "{labels.2}.localhost"

		dynamic a {
			name "{labels.2}"
			port 80
			resolvers dns
		}
	}

	handle_errors {
		@404 expression {err.message} == "no upstreams available"
		reverse_proxy @404 {
			to localhost:80

			header_up Host 404.localhost
		}
	}
}
```

## Contributing

Contributions are welcome! Please fork the repository and submit a pull request.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

## Contact

For any questions or suggestions, please open an issue or contact the repository owner.
