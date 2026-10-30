use crate::components::{KeysPage, LoginPage, ModelsPage, PolicyPage, RequestsPage};
use leptos::prelude::*;
use leptos_meta::{Stylesheet, Title, provide_meta_context};
use leptos_router::{
  components::{Redirect, Route, Router, Routes},
  path,
};

/// Shared server/browser component tree. Services remain behind server functions.
#[component]
pub fn App() -> impl IntoView {
  provide_meta_context();
  view! {
    <Stylesheet id="gateway-theme" href="/ui/assets/app.css"/>
    <Title text="Lyonbrown Gateway"/>
    <Router>
      <Routes fallback=|| view! { <main><h1>"页面不存在"</h1><a href="/ui/models">"返回模型目录"</a></main> }>
        <Route path=path!("/ui") view=|| view! { <Redirect path="/ui/models"/> }/>
        <Route path=path!("/ui/login") view=LoginPage/>
        <Route path=path!("/ui/models") view=ModelsPage/>
        <Route path=path!("/ui/policy") view=PolicyPage/>
        <Route path=path!("/ui/requests") view=RequestsPage/>
        <Route path=path!("/ui/keys") view=KeysPage/>
      </Routes>
    </Router>
  }
}
