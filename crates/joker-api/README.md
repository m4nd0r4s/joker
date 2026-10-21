# joker-api

The local HTTP API of `jokerd` (ref/09, `spec/openapi/joker-api.yaml`): the routes of the M0 subset, the strict request types, RFC 9457 problems, and the server over the Unix socket. The daemon implements [`Backend`]; `joker-client` reuses the request types.
