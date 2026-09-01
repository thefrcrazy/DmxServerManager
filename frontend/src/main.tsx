import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
// Inter est auto-hébergée : le jeton la déclarait depuis l'origine sans qu'aucune
// @font-face n'existe, si bien que l'application retombait sur la police système
// et changeait d'aspect selon macOS, Windows ou Android. L'axe de graisse seul
// suffit ; le navigateur ne télécharge que le sous-ensemble latin.
import "@fontsource-variable/inter/wght.css";
import "./styles/main.scss";
import App from "./App";

ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
        <BrowserRouter>
            <App />
        </BrowserRouter>
    </React.StrictMode>
);
