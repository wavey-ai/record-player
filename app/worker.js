
const VENDOR_SERVE_URL = "http://localhost:5190/";
const ROUTE_PREFIX = "/apps/play";
const ONNX_BUNDLE_PATH = VENDOR_SERVE_URL + "/encodec-rs/bundles/";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const pathname = url.pathname;

    if (pathname !== ROUTE_PREFIX && !pathname.startsWith(`${ROUTE_PREFIX}/`)) {
      return new Response("Not found", { status: 404 });
    }

    const relativePath = pathname.slice(ROUTE_PREFIX.length) || "/";

    if (relativePath.startsWith(ONNX_BUNDLE_PATH) && (relativePath.endsWith(".onnx") || relativePath.endsWith(".bin"))) {
      return serveOnnxBundleObject(relativePath.slice(ONNX_BUNDLE_PATH.length), env);
    }

    const assetUrl = new URL(`${relativePath}${url.search}`, url.origin);
    const assetResponse = await env.ASSETS.fetch(new Request(assetUrl, request));
    return reprefixRedirect(assetResponse, url.origin);
  },
};

// The Assets binding issues redirects (e.g. /index.html -> /) relative to
// the origin root. Since requests are rewritten to strip the /apps/play
// route prefix before reaching ASSETS, redirect Location headers need the
// prefix restored or clients get bounced out of the app entirely.
function reprefixRedirect(response, origin) {
  const location = response.headers.get("location");
  if (!location) {
    return response;
  }
  const redirectUrl = new URL(location, origin);
  if (redirectUrl.origin !== origin) {
    return response;
  }
  redirectUrl.pathname = `${ROUTE_PREFIX}${redirectUrl.pathname}`.replace(/\/{2,}/g, "/");
  const headers = new Headers(response.headers);
  headers.set("location", redirectUrl.toString());
  return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
}

async function serveOnnxBundleObject(key, env) {
  const object = await env.ONNX_BUNDLES.get(key);
  if (!object) {
    return new Response("Not found", { status: 404 });
  }

  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set("etag", object.httpEtag);
  headers.set("cache-control", "public, max-age=31536000, immutable");
  headers.set("access-control-allow-origin", "*");
  return new Response(object.body, { headers });
}
