extern crate prost_build;
extern crate proc_macro2;

use prost_build::{Method, Service, ServiceGenerator};
use proc_macro2::{TokenStream, Ident, Span, Literal};
use std::fmt::Write;

#[derive(Default)]
pub struct TwirpServiceGenerator {
    pub embed_client: bool,
    type_aliases_generated: bool,
}

impl TwirpServiceGenerator {
    pub fn new() -> Self { Default::default() }

    fn ident(&self, id: &str) -> Ident {
        Ident::new(id, Span::call_site())
    }

    fn service_name(&self, service: &Service) -> Ident {
        self.ident(&service.name)
    }

    fn client_name(&self, service: &Service) -> Ident {
        self.ident(&format!("{}Client", service.name))
    }

    fn server_name(&self, service: &Service) -> Ident {
        self.ident(&format!("{}Server", service.name))
    }

    fn twirp_uri(&self, service: &Service, method: &Method) -> Literal {
        Literal::string(&format!("/twirp/{}.{}/{}", service.package, service.proto_name, method.proto_name))
    }

    fn twirp_mod(&self) -> TokenStream {
        let modname = Ident::new("twirp_rs", Span::call_site());
        if self.embed_client {
            quote!{ #modname }
        } else {
            quote!{ ::#modname }
        }
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
            pub trait #name {
                #( #methods; )*
            }
        }
    }

    fn generate_main_impl(&self, service: &Service) -> TokenStream {
        let name = self.service_name(service);
        let server_name = self.server_name(service);
        let client_name = self.client_name(service);
        let module = self.twirp_mod();

        quote! {
            impl #name {
                pub fn new_client(client: ::hyper::Client<::hyper::client::HttpConnector, ::hyper::Body>, root_url: &str) -> Box<#name> {
                    Box::new(#client_name(#module::HyperClient::new(client, root_url)))
                }

                pub fn new_server<T: 'static + #name>(v: T) ->
                    Box<::hyper::service::Service<
                        ReqBody=::hyper::Body,
                        ResBody=::hyper::Body,
                        Error=::hyper::Error,
                        Future=Box<::futures::Future<Item=::hyper::Response<::hyper::Body>, Error=::hyper::Error>>
                    >> {
                    Box::new(#module::HyperServer::new(#server_name(::std::sync::Arc::new(v))))
                }
            }
        }
    }

    fn generate_client_struct(&self, service: &Service) -> TokenStream {
        let client_name = self.client_name(service);
        let module = self.twirp_mod();

        quote! {
            pub struct #client_name(pub #module::HyperClient);
        }
    }

    fn client_method(&self, service: &Service, method: &Method) -> TokenStream {
        let signature = self.method_sig(method);
        let uri = self.twirp_uri(service, method);

        quote! {
            #signature {
                self.0.go(#uri, i)
            }
        }
    }

    fn generate_client_impl(&self, service: &Service) -> TokenStream {
        let name = self.service_name(service);
        let client_name = self.client_name(service);
        let methods = service.methods.iter().map(|method| self.client_method(service, method));

        quote! {
            impl #name for #client_name {
                #( #methods )*
            }
        }
    }

    fn server_handler(&self, service: &Service, method: &Method) -> TokenStream {
        let uri = self.twirp_uri(service, method);
        let method = self.ident(&method.name);

        quote! {
            (::hyper::Method::POST, #uri) =>
                Box::new(::futures::future::result(req.to_proto()).and_then(move |v| static_service.#method(v)).and_then(|v| v.to_proto_raw()))
        }
    }

    fn generate_server_impl(&self, service: &Service) -> TokenStream {
        let module = self.twirp_mod();
        let name = self.service_name(service);
        let server_name = self.server_name(service);
        let handlers = service.methods.iter().map(|method| self.server_handler(service, method));

        quote! {
            impl<T: 'static + #name> #module::HyperService for #server_name<T> {
                fn handle(&self, req: #module::ServiceRequest<Vec<u8>>) -> #module::PTRes<Vec<u8>> {
                    use ::futures::Future;
                    let static_service = self.0.clone();
                    match (req.method.clone(), req.uri.path()) {
                        #( #handlers, )*
                        _ => Box::new(::futures::future::ok(#module::TwirpError::new(::hyper::StatusCode::NOT_FOUND, "not_found", "Not found").to_resp_raw())),
                    }
                }
            }
        }
    }

    fn generate_server_struct(&self, service: &Service) -> TokenStream {
        let name = self.service_name(service);
        let server_name = self.server_name(service);

        quote! {
            pub struct #server_name<T: 'static + #name>(::std::sync::Arc<T>);
        }
    }
}

impl ServiceGenerator for TwirpServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        let mut tokens = TokenStream::new();

        tokens.extend(self.generate_type_aliases());
        tokens.extend(self.generate_main_trait(&service));
        tokens.extend(self.generate_main_impl(&service));

        tokens.extend(self.generate_client_struct(&service));
        tokens.extend(self.generate_client_impl(&service));

        tokens.extend(self.generate_server_struct(&service));
        tokens.extend(self.generate_server_impl(&service));

        write!(buf, "{}", &tokens).unwrap();
    }

    fn finalize(&mut self, buf: &mut String) {
    }
}
