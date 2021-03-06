use futures::{Future, Stream};
use futures::future;
use hyper;
use hyper::{Body, Client, HeaderMap, Version, Method, Request, Response, StatusCode, Uri};
use hyper::client::HttpConnector;
use hyper::header::{HeaderValue, CONTENT_TYPE, CONTENT_LENGTH};
use prost::{DecodeError, EncodeError, Message};
use serde_derive::{Serialize, Deserialize};

pub type FutReq<T> = Box<Future<Item=ServiceRequest<T>, Error=ProstTwirpError> + Send>;

/// The type of every service request 
pub type PTReq<I> = ServiceRequest<I>;

/// The type of every service response
pub type PTRes<O> = Box<Future<Item=ServiceResponse<O>, Error=ProstTwirpError> + Send>;

/// A request with HTTP info and the serialized input object
#[derive(Debug)]
pub struct ServiceRequest<T> {
    /// The URI of the original request
    /// 
    /// When using a client, this will be overridden with the proper URI. It is only valuable for servers.
    pub uri: Uri,
    /// The request method; should always be Post
    pub method: Method,
    /// The HTTP version, rarely changed from the default
    pub version: Version,
    /// The set of headers
    ///
    /// Should always at least have `Content-Type`. Clients will override `Content-Length` on serialization.
    pub headers: HeaderMap<HeaderValue>,
    // The serialized request object
    pub input: T,
}

fn application_proto() -> HeaderValue {
    HeaderValue::from_static("application/protobuf")
}

fn application_json() -> HeaderValue {
    HeaderValue::from_static("application/json")
}

impl<T> ServiceRequest<T> {
    /// Create new service request with the given input object
    /// 
    /// This automatically sets the `Content-Type` header as `application/protobuf`.
    pub fn new(input: T) -> ServiceRequest<T> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, application_proto());
        ServiceRequest {
            uri: Default::default(),
            method: Method::POST,
            version: Version::default(),
            headers: headers,
            input
        }
    }
    
    /// Copy this request with a different input value
    pub fn clone_with_input<U>(&self, input: U) -> ServiceRequest<U> {
        ServiceRequest { uri: self.uri.clone(), method: self.method.clone(), version: self.version,
            headers: self.headers.clone(), input }
    }
}

impl<T: Message + Default + 'static> From<T> for ServiceRequest<T> {
    fn from(v: T) -> ServiceRequest<T> { ServiceRequest::new(v) }
}

impl ServiceRequest<Vec<u8>> {
    /// Turn a hyper request to a boxed future of a byte-array service request
    pub fn from_hyper_raw(req: Request<Body>) -> FutReq<Vec<u8>> {
        let uri = req.uri().clone();
        let method = req.method().clone();
        let version = req.version();
        let headers = req.headers().clone();
        Box::new(req.into_body().concat2().map_err(ProstTwirpError::HyperError).map(move |body| {
            ServiceRequest { uri, method, version, headers, input: body.to_vec() }
        }))
    }

    /// Turn a byte-array service request into a hyper request
    pub fn to_hyper_raw(&self) -> Request<Body> {
        let mut req = Request::builder()
            .method("POST")
            .uri(self.uri.clone())
            .body(Body::from(self.input.clone()))
            .unwrap();

        req.headers_mut().clone_from(&self.headers);
        req.headers_mut().insert(CONTENT_LENGTH, HeaderValue::from(self.input.len() as u64));
        req
    }

    /// Turn a byte-array service request into a `AfterBodyError`-wrapped version of the given error
    pub fn body_err(&self, err: ProstTwirpError) -> ProstTwirpError {
        ProstTwirpError::AfterBodyError {
            body: self.input.clone(), method: Some(self.method.clone()), version: self.version,
            headers: self.headers.clone(), status: None, err: Box::new(err)
        }
    }

    /// Serialize the byte-array service request into a protobuf service request
    pub fn to_proto<T: Message + Default + 'static>(&self) -> Result<ServiceRequest<T>, ProstTwirpError> {
        match T::decode(&self.input) {
            Ok(v) => Ok(self.clone_with_input(v)),
            Err(err) => Err(self.body_err(ProstTwirpError::ProstDecodeError(err)))
        }
    }
}

impl<T: Message + Default + 'static> ServiceRequest<T> {
    /// Turn a protobuf service request into a byte-array service request
    pub fn to_proto_raw(&self) -> Result<ServiceRequest<Vec<u8>>, ProstTwirpError> {
        let mut body = Vec::new();
        if let Err(err) = self.input.encode(&mut body) {
            Err(ProstTwirpError::ProstEncodeError(err))
        } else {
            Ok(self.clone_with_input(body))
        }
    }

    /// Turn a hyper request into a protobuf service request
    pub fn from_hyper_proto(req: Request<Body>) -> FutReq<T> {
        Box::new(ServiceRequest::from_hyper_raw(req).and_then(|v| v.to_proto()))
    }

    /// Turn a protobuf service request into a hyper request
    pub fn to_hyper_proto(&self) -> Result<Request<Body>, ProstTwirpError> {
        self.to_proto_raw().map(|v| v.to_hyper_raw())
    }
}

/// A response with HTTP info and a serialized output object
#[derive(Debug)]
pub struct ServiceResponse<T> {
    /// The HTTP version
    pub version: Version,
    /// The set of headers
    ///
    /// Should always at least have `Content-Type`. Servers will override `Content-Length` on serialization.
    pub headers: HeaderMap<HeaderValue>,
    /// The status code
    pub status: StatusCode,
    /// The serialized output object
    pub output: T,
}

impl<T> ServiceResponse<T> {
    /// Create new service request with the given input object
    /// 
    /// This automatically sets the `Content-Type` header as `application/protobuf`.
    pub fn new(output: T) -> ServiceResponse<T> { 
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", application_proto());
        ServiceResponse {
            version: Version::default(),
            headers: headers,
            status: StatusCode::OK,
            output
        }
    }
    
    /// Copy this response with a different output value
    pub fn clone_with_output<U>(&self, output: U) -> ServiceResponse<U> {
        ServiceResponse { version: self.version, headers: self.headers.clone(), status: self.status, output }
    }
}

impl<T: Message + Default + 'static> From<T> for ServiceResponse<T> {
    fn from(v: T) -> ServiceResponse<T> { ServiceResponse::new(v) }
}

impl ServiceResponse<Vec<u8>> {
    /// Turn a hyper response to a boxed future of a byte-array service response
    pub fn from_hyper_raw(resp: Response<Body>) -> PTRes<Vec<u8>> {
        let version = resp.version();
        let headers = resp.headers().clone();
        let status = resp.status();
        Box::new(resp.into_body().concat2().map_err(ProstTwirpError::HyperError).map(move |body| {
            ServiceResponse { version, headers, status, output: body.to_vec() }
        }))
    }

    /// Turn a byte-array service response into a hyper response
    pub fn to_hyper_raw(&self) -> Response<Body> {
        let mut res = Response::builder()
            .status(self.status)
            .body(Body::from(self.output.clone()))
            .unwrap();

        res.headers_mut().clone_from(&self.headers);
        res.headers_mut().insert(CONTENT_LENGTH, HeaderValue::from(self.output.len() as u64));
        res
    }

    /// Turn a byte-array service response into a `AfterBodyError`-wrapped version of the given error
    pub fn body_err(&self, err: ProstTwirpError) -> ProstTwirpError {
        ProstTwirpError::AfterBodyError {
            body: self.output.clone(), method: None, version: self.version,
            headers: self.headers.clone(), status: Some(self.status), err: Box::new(err)
        }
    }

    /// Serialize the byte-array service response into a protobuf service response
    pub fn to_proto<T: Message + Default + 'static>(&self) -> Result<ServiceResponse<T>, ProstTwirpError> {
        if self.status.is_success() {
            match T::decode(&self.output) {
                Ok(v) => Ok(self.clone_with_output(v)),
                Err(err) => Err(self.body_err(ProstTwirpError::ProstDecodeError(err)))
            }
        } else {
            match TwirpError::from_json_bytes(self.status, &self.output) {
                Ok(err) => Err(self.body_err(ProstTwirpError::TwirpError(err))),
                Err(err) => Err(self.body_err(ProstTwirpError::JsonDecodeError(err)))
            }
        }
    }
}

impl<T: Message + Default + 'static> ServiceResponse<T> {
    /// Turn a protobuf service response into a byte-array service response
    pub fn to_proto_raw(&self) -> Result<ServiceResponse<Vec<u8>>, ProstTwirpError> {
        let mut body = Vec::new();
        if let Err(err) = self.output.encode(&mut body) {
            Err(ProstTwirpError::ProstEncodeError(err))
        } else {
            Ok(self.clone_with_output(body))
        }
    }

    /// Turn a hyper response into a protobuf service response
    pub fn from_hyper_proto(resp: Response<Body>) -> PTRes<T> {
        Box::new(ServiceResponse::from_hyper_raw(resp).and_then(|v| v.to_proto()))
    }

    /// Turn a protobuf service response into a hyper response
    pub fn to_hyper_proto(&self) -> Result<Response<Body>, ProstTwirpError> {
        self.to_proto_raw().map(|v| v.to_hyper_raw())
    }
}

/// A JSON-serializable Twirp error
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct TwirpError {
    #[serde(skip)]
    pub status: StatusCode,
    pub code: String,
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

impl TwirpError {
    /// Create a Twirp error with no meta
    pub fn new(status: StatusCode, code: &str, msg: &str) -> TwirpError {
        TwirpError::new_meta(status, code, msg, None)
    }

    /// Create a Twirp error with optional meta
    pub fn new_meta(status: StatusCode, error_type: &str, msg: &str, meta: Option<serde_json::Value>) -> TwirpError {
        TwirpError { status, code: error_type.to_string(), msg: msg.to_string(), meta }
    }

    /// Create a byte-array service response for this error and the given status code
    pub fn to_resp_raw(&self) -> ServiceResponse<Vec<u8>> {
        let output = self.to_json_bytes().unwrap_or_else(|_| "{}".as_bytes().to_vec());
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, application_json());
        headers.insert(CONTENT_LENGTH, HeaderValue::from(output.len() as u64));
        ServiceResponse {
            version: Version::default(),
            headers: headers,
            status: self.status,
            output
        }
    }

    /// Create a hyper response for this error and the given status code
    pub fn to_hyper_resp(&self) -> Response<Body> {
        let body = self.to_json_bytes().unwrap_or_else(|_| "{}".as_bytes().to_vec());
        Response::builder().
            status(self.status).
            header(CONTENT_TYPE, application_json()).
            header(CONTENT_LENGTH, body.len() as u64).
            body(Body::from(body)).unwrap()
    }

    /// Create error from byte array
    pub fn from_json_bytes(status: StatusCode, json: &[u8]) -> serde_json::Result<TwirpError> {
        serde_json::from_slice(json).map(|err| TwirpError{ status, ..err })
    }

    /// Create byte array from error
    pub fn to_json_bytes(&self) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(&self)
    }
}

impl From<TwirpError> for ProstTwirpError {
    fn from(v: TwirpError) -> ProstTwirpError { ProstTwirpError::TwirpError(v) }
}

/// An error that can occur during a call to a Twirp service
#[derive(Debug)]
pub enum ProstTwirpError {
    /// A standard Twirp error with a type, message, and some metadata
    TwirpError(TwirpError),
    /// An error when trying to decode JSON into an error or object
    JsonDecodeError(serde_json::Error),
    /// An error when trying to encode a protobuf object
    ProstEncodeError(EncodeError),
    /// An error when trying to decode a protobuf object
    ProstDecodeError(DecodeError),
    /// A generic hyper error
    HyperError(hyper::Error),

    /// A wrapper for any of the other `ProstTwirpError`s that also includes request/response info
    AfterBodyError {
        /// The request or response's raw body before the error happened
        body: Vec<u8>,
        /// The request method, only present for server errors
        method: Option<Method>,
        /// The request or response's HTTP version
        version: Version,
        /// The request or response's headers
        headers: HeaderMap<HeaderValue>,
        /// The response status, only present for client errors
        status: Option<StatusCode>,
        /// The underlying error
        err: Box<ProstTwirpError>,
    }
}

impl ProstTwirpError {
    /// This same error, or the underlying error if it is an `AfterBodyError`
    pub fn root_err(self) -> ProstTwirpError {
        match self {
            ProstTwirpError::AfterBodyError { err, .. } => err.root_err(),
            _ => self
        }
    }

    pub fn to_hyper_resp(self) -> Result<Response<Body>, hyper::Error> {
        match self.root_err() {
            ProstTwirpError::ProstDecodeError(_) =>
                Ok(TwirpError::new(StatusCode::BAD_REQUEST, "protobuf_decode_err", "Invalid protobuf body").
                    to_hyper_resp()),
            ProstTwirpError::TwirpError(err) =>
                Ok(err.to_hyper_resp()),
            // Just propagate hyper errors
            ProstTwirpError::HyperError(err) =>
                Err(err),
            _ =>
                Ok(TwirpError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_err", "Internal Error").
                    to_hyper_resp()),
        }
    }
}

#[cfg(test)]
mod twirp_error_tests {
    use super::*;

    fn default_error() -> TwirpError {
        TwirpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal".to_string(),
            msg: "Something went wrong".to_string(),
            meta: None,
        }
    }

    fn default_json() -> &'static str {
        r#"{"code":"internal","msg":"Something went wrong"}"#
    }

    #[test]
    fn serialization() {
        let err = default_error();
        let json = TwirpError::to_json_bytes(&err).unwrap();
        assert_eq!(String::from_utf8(json).unwrap(), default_json());
    }

    #[test]
    fn deserialization() {
        let err = TwirpError::from_json_bytes(StatusCode::INTERNAL_SERVER_ERROR, default_json().as_bytes());
        assert_eq!(err.unwrap(), default_error());
    }
}

/// A wrapper for a hyper client
#[derive(Debug)]
pub struct HyperClient {
    /// The hyper client
    pub client: Client<HttpConnector, Body>,
    /// The root URL without any path attached
    pub root_url: String,
}

impl HyperClient {
    /// Create a new client wrapper for the given client and root using protobuf
    pub fn new(client: Client<HttpConnector, Body>, root_url: &str) -> HyperClient {
        HyperClient {
            client,
            root_url: root_url.trim_right_matches('/').to_string(),
        }
    }

    /// Invoke the given request for the given path and return a boxed future result
    pub fn go<I, O>(&self, path: &str, req: ServiceRequest<I>) -> PTRes<O>
            where I: Message + Default + 'static, O: Message + Default + 'static {
        // Build the URI
        let uri = format!("{}/{}", self.root_url, path.trim_left_matches('/')).parse().unwrap();

        // Build the request
        let mut hyper_req = match req.to_hyper_proto() {
            Err(err) => return Box::new(future::err(err)),
            Ok(v) => v
        };
        *hyper_req.uri_mut() = uri;

        // Run the request and map the response
        Box::new(self.client.request(hyper_req).
            map_err(ProstTwirpError::HyperError).
            and_then(ServiceResponse::from_hyper_proto))
    }
}

