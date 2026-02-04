import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
    plugins: [react()],
    resolve: {
        alias: {
            "@": path.resolve(__dirname, "./src"),
        },
    },
    server: {
        port: 3000,
        proxy: {
            "/api": {
                target: process.env.VITE_BACKEND_HTTPS === "true" 
                    ? "https://localhost:5500" 
                    : "http://localhost:5500",
                changeOrigin: true,
                secure: false, // Essential for self-signed certificates
                ws: true,
            },
        },
    },
    css: {
        preprocessorOptions: {
            scss: {
                additionalData: "@use \"@/styles/_variables\" as *; @use \"@/styles/_mixins\" as *;",
            },
        },
    },
});
