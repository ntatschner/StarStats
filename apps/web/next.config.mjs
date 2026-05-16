// Packages that must NEVER be bundled by webpack on the server side.
// `@grpc/grpc-js` does `require('stream')` in plain CommonJS that
// webpack's resolver chokes on, and the OTel SDK chain pulls it in
// transitively from `instrumentation.ts`. `serverExternalPackages`
// covers Server Components / Route Handlers, but the instrumentation
// bundle uses a separate webpack config — hence the explicit
// `config.externals` push in the `webpack` callback below.
const otelExternals = [
  '@opentelemetry/sdk-node',
  '@opentelemetry/exporter-trace-otlp-grpc',
  '@opentelemetry/auto-instrumentations-node',
  '@opentelemetry/resources',
  '@opentelemetry/semantic-conventions',
  '@grpc/grpc-js',
  // `@sentry/node` (used in `instrumentation.ts` to ship errors to
  // GlitchTip) pulls native Node deps for source-map handling that
  // webpack can't resolve. Same treatment as the OTel stack.
  '@sentry/node',
  // pino's transport mechanism (used by `lib/logger.ts` to wire
  // pino-pretty in dev) spawns a worker thread that dynamically
  // requires the target module at runtime. Next.js's bundler can't
  // see that dynamic require, so without externalising pino +
  // pino-pretty the worker tries to load
  // `.next/server/vendor-chunks/lib/worker.js` which never gets
  // emitted, and the dev server crashes on the first server-side
  // log call (which Playwright trips immediately via TopBar's
  // location fetch).
  'pino',
  'pino-pretty',
];

// Baseline security headers applied to every response. Traefik in front
// of this app may layer additional headers (HSTS preload, CSP); the
// values here are the safe defaults that don't depend on the deployment
// topology.
const securityHeaders = [
  {
    key: 'Strict-Transport-Security',
    value: 'max-age=31536000; includeSubDomains',
  },
  { key: 'X-Frame-Options', value: 'DENY' },
  { key: 'X-Content-Type-Options', value: 'nosniff' },
  { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
  {
    key: 'Permissions-Policy',
    value: 'camera=(), microphone=(), geolocation=(), interest-cohort=()',
  },
];

/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'standalone',
  poweredByHeader: false,
  reactStrictMode: true,
  experimental: {
    typedRoutes: true,
  },
  serverExternalPackages: otelExternals,
  async headers() {
    return [
      {
        source: '/:path*',
        headers: securityHeaders,
      },
    ];
  },
  // Route renames from the design audit v2 (§07). Redirects keep
  // existing bookmarks / outbound links from this project's release
  // notes / Revolut webhook return URLs working after the move.
  async redirects() {
    return [
      { source: '/donate', destination: '/support', permanent: true },
      { source: '/donate/return', destination: '/support/return', permanent: true },
    ];
  },
  webpack: (config, { isServer }) => {
    if (isServer) {
      const externals = config.externals;
      if (Array.isArray(externals)) {
        externals.push(...otelExternals);
      } else if (externals !== undefined) {
        config.externals = [externals, ...otelExternals];
      } else {
        config.externals = [...otelExternals];
      }
    }
    return config;
  },
};

export default nextConfig;
