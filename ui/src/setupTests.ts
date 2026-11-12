// Los matchers de jest-dom (`toBeInTheDocument`, …) se registran en `expect` de
// Vitest: la entrada `/vitest` es la que declara la augmentación de tipos (la
// raíz del paquete augmenta el `expect` de Jest y no compila aquí).
import '@testing-library/jest-dom/vitest'
