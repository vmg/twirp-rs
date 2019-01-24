use prost_build::{Method, Service, ServiceGenerator};
use proc_macro2::{TokenStream, Ident, Span, Literal};
use std::fmt::Write;
use std::process::{Command, Stdio};
use quote::quote;

#[derive(Default)]
pub struct TwirpServiceGenerator {
    pub generate_client: bool,
    pub generate_server: bool,
}

impl TwirpServiceGenerator {
    pub fn new() -> Self {
        TwirpServiceGenerator {
            generate_client: false,
            generate_server: true
        }
    }

    #[allow(dead_code)]
    fn comment(&self, comment: &str) -> TokenStream {
        use std::str::FromStr;
        TokenStream::from_str(comment).unwrap()
    }

    fn ident(&self, id: &str) -> Ident {
        Ident::new(id, Span::call_site())
    }

    fn service_name(&self, service: &Service) -> Ident {
        self.ident(&service.name)
    }

    fn twirp_uri(&self, service: &Service, method: &Method) -> Literal {
        Literal::string(&format!("/twirp/{}.{}/{}", service.package, service.proto_name, method.proto_name))
    }

    fn twirp_mod(&self) -> TokenStream {
        let modname = Ident::new("twirp_rs", Span::call_site());
        quote!{ ::#modname }
    }

    fn generate_type_aliases(&self) -> TokenStream {
        let module = self.twirp_mod();

        quote! {
            pub type PTReq<I> = #module::PTReq<I>;
            pub type PTRes<O> = #module::PTRes<O>;
        }
    }

    fn method_sig(&self, method: &Method) -> TokenStream {
        let name = self.ident(&method.name);
        let module = self.twirp_mod();
        let input_type = self.ident(&method.input_type);
        let output_type = self.ident(&method.output_type);

        quote! {
            fn #name(&self, i: #module::PTReq<#input_type>) -> #module::PTRes<#output_type>
        }
    }

    fn generate_main_trait(&self, service: &Service) -> TokenStream {
        let name = self.service_name(service);
        let methods = service.methods.iter().map(|method| self.method_sig(method));

        quote! {
            pub trait #name: Send {
                #( #methods; )*
            }
        }
    }

    fn generate_client(&self, service: &Service) -> TokenStream {
        let module = self.twirp_mod();
        let name = self.service_name(service);
        let client_name = self.ident(&format!("{}Client", service.name));

        let methods = service.methods.iter().map(|method| {
            let signature = self.method_sig(method);
            let uri = self.twirp_uri(service, method);

            quote! {
                #signature {
                    self.0.go(#uri, i)
                }
            }
        });

        quote! {
            pub struct #client_name(pub #module::HyperClient);

            impl #name {
                pub fn client(client: ::hyper::Client<::hyper::client::HttpConnector, ::hyper::Body>, root_url: &str) -> Box<#name> {
                    Box::new(#client_name(#module::HyperClient::new(client, root_url)))
                }
            }

            impl #name for #client_name {
                #( #methods )*
            }
        }
    }

    fn generate_http_handler(&self, service: &Service) -> TokenStream {
        let name = self.service_name(service);
        let module = self.twirp_mod();

        let handlers = service.methods.iter().map(|method| {
            let uri = self.twirp_uri(service, method);
            let method = self.ident(&method.name);

            quote! {
                (Method::POST, #uri) => { Box::new(future::result(req.to_proto()).and_then(move |v| service.#method(v)).and_then(|v| v.to_hyper_proto())) }
            }
        });

        quote! {
            impl #name {
                pub fn server_handler<T: 'static + #name>(service: T, req: ::hyper::Request<::hyper::Body>) ->
                    Box<::futures::Future<Item = ::hyper::Response<::hyper::Body>, Error = ::hyper::Error> + Send>
                {
                    use ::futures::{future, Future};
                    use #module::{TwirpError, ProstTwirpError};
                    use ::hyper::{StatusCode, Response, Body, Method};
                    type ResponseFuture = Box<Future<Item=Response<Body>, Error=ProstTwirpError> + Send>;

                    match req.headers().get(::hyper::header::CONTENT_TYPE) {
                        Some(ct) if ct == "application/protobuf" => (),
                        Some(ct) if ct == "application/json" => (),
                        _ => {
                            return Box::new(future::ok(TwirpError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE,
                                "bad_content_type", "Content type must be application/protobuf").to_hyper_resp()))
                        }
                    }

                    Box::new(
                        #module::ServiceRequest::from_hyper_raw(req).and_then(move |req| -> ResponseFuture {
                            match (req.method.clone(), req.uri.path()) {
                                #( #handlers, )*
                                _ => { Box::new(future::ok(TwirpError::new(StatusCode::NOT_FOUND, "not_found", "RPC Path not found").to_hyper_resp())) }
                            }
                        }).or_else(|err| err.to_hyper_resp())
                    )
                }
            }
        }
    }
}

impl TwirpServiceGenerator {
    fn render(&self, tokens: TokenStream, buf: &mut String) {
        match TwirpServiceGenerator::rustfmt(&tokens) {
            Ok(formatted) => buf.write_str(&formatted).unwrap(),
            Err(_) => write!(buf, "{}", &tokens).unwrap(),
        }
    }


    fn rustfmt(input: &TokenStream) -> Result<String, String> {
        use std::io::Write;

        let mut rustfmt = Command::new("rustfmt")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|_| format!("Couldn't spawn rustfmt"))?;

        {
            let stdin = rustfmt
                .stdin
                .as_mut()
                .ok_or_else(|| "Failed to open rustfmt stdin".to_string())?;

            stdin.write_all(input.to_string().as_bytes())
                .expect("failed to write input into rustfmt");
        }

        rustfmt
            .wait_with_output()
            .map_err(|err| format!("Error running rustfmt: {}", err))
            .and_then(|out| {
                String::from_utf8(out.stdout)
                    .map_err(|_| "Formatted code is not valid UTF-8".to_string())
            })
    }
}


impl ServiceGenerator for TwirpServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        let mut tokens = TokenStream::new();

        tokens.extend(self.generate_type_aliases());
        tokens.extend(self.generate_main_trait(&service));
        if self.generate_client {
            tokens.extend(self.generate_client(&service));
        }
        if self.generate_server {
            // tokens.extend(self.generate_server_impl(&service));
            tokens.extend(self.generate_http_handler(&service));
        }

        self.render(tokens, buf);
    }
}
