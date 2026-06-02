import axios from "axios"
import { getKey } from "@/lib/auth"
import { handleUnauthorized } from "@/lib/queryClient"

const BASE = import.meta.env.VITE_API_URL ?? "http://localhost:8080"

export const api = axios.create({ baseURL: `${BASE}/api/v1` })

api.interceptors.request.use((config) => {
  const key = getKey()
  if (key) config.headers.Authorization = `Bearer ${key}`
  return config
})

api.interceptors.response.use(
  (res) => res,
  (err) => {
    if (err.response?.status === 401) handleUnauthorized()
    return Promise.reject(err)
  }
)
