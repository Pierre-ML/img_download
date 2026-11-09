// Animaux low-poly de l'univers Lucania, viewBox 0 0 64 64.
// Format : facets = polygones { points, opacity } ; eyes = cercles { cx, cy, r } ;
// accents = petits triangles { points }. Tout est rendu en currentColor (aucun rouge).
// hibou, corbeau, renard, aigle et cerf sont copiés à l'identique de Lucania ; la pie est propre à ce logiciel.
//
// Table des rôles :
//   pie     = logo + mascotte (l'oiseau qui collectionne ce qui brille)
//   hibou   = Google Images
//   corbeau = Likes X
//   renard  = Reddit enregistrés
//   aigle   = Comptes
//   cerf    = Réglages
type Point = [number, number];
export interface Animal {
  id: string;
  set: string;
  facets: { points: Point[]; opacity: number }[];
  eyes: { cx: number; cy: number; r: number }[];
  accents: { points: Point[] }[];
}

export const animals: Record<string, Animal> = {
  pie: {
    id: 'pie',
    set: 'minimal',
    facets: [
      { points: [[33,13],[40,4],[50,7],[44,13]], opacity: 0.9 },
      { points: [[33,13],[44,13],[50,7],[51,17],[43,21],[35,20]], opacity: 0.4 },
      { points: [[50,7],[61,15],[51,17]], opacity: 0.6 },
      { points: [[51,17],[61,15],[48,19.5]], opacity: 0.3 },
      { points: [[35,20],[43,21],[49,24],[47,35],[38,41]], opacity: 1 },
      { points: [[35,20],[26,27],[38,41]], opacity: 0.7 },
      { points: [[26,27],[38,41],[32,44],[14,36],[17,30]], opacity: 0.45 },
      { points: [[38,41],[32,44],[36,49]], opacity: 0.5 },
      { points: [[14,36],[32,44],[2,60]], opacity: 0.7 },
      { points: [[14,36],[2,60],[7,41]], opacity: 0.3 },
    ],
    eyes: [{ cx: 45, cy: 16, r: 1.8 }],
    accents: [],
  },
  corbeau: {
    id: 'corbeau',
    set: 'minimal',
    facets: [
      { points: [[33,19],[38,9],[47,5],[52,12],[49,14]], opacity: 0.85 },
      { points: [[33,19],[49,14],[49,24],[41,27]], opacity: 1 },
      { points: [[49,14],[63,28],[49,24]], opacity: 0.5 },
      { points: [[49,24],[63,28],[47,31]], opacity: 0.3 },
      { points: [[41,27],[49,24],[47,31],[52,42],[44,50]], opacity: 0.45 },
      { points: [[33,19],[41,27],[12,30]], opacity: 0.45 },
      { points: [[12,30],[41,27],[44,50],[30,54]], opacity: 0.7 },
      { points: [[12,30],[30,54],[3,61]], opacity: 0.3 },
      { points: [[12,30],[3,61],[4,40]], opacity: 0.5 },
    ],
    eyes: [{ cx: 43, cy: 18.5, r: 2 }],
    accents: [],
  },
  hibou: {
    id: 'hibou',
    set: 'minimal',
    facets: [
      { points: [[10,8],[32,22],[32,58],[10,40]], opacity: 0.6 },
      { points: [[54,8],[32,22],[32,58],[54,40]], opacity: 0.35 },
      { points: [[28,32],[36,32],[32,44]], opacity: 1 },
    ],
    eyes: [{ cx: 21, cy: 28, r: 5 }, { cx: 43, cy: 28, r: 5 }],
    accents: [],
  },
  renard: {
    id: 'renard',
    set: 'minimal',
    facets: [
      { points: [[5,3],[28,26],[7,31]], opacity: 0.7 },
      { points: [[4,37],[7,31],[28,26],[32,40]], opacity: 0.5 },
      { points: [[4,37],[32,40],[20,47]], opacity: 0.3 },
      { points: [[20,47],[32,40],[32,60]], opacity: 0.6 },
      { points: [[57,31],[36,26],[59,3]], opacity: 0.45 },
      { points: [[32,40],[36,26],[57,31],[60,37]], opacity: 0.3 },
      { points: [[44,47],[32,40],[60,37]], opacity: 0.2 },
      { points: [[32,60],[32,40],[44,47]], opacity: 0.4 },
      { points: [[28,26],[36,26],[32,40]], opacity: 1 },
    ],
    eyes: [{ cx: 19, cy: 34, r: 1.6 }, { cx: 45, cy: 34, r: 1.6 }],
    accents: [{ points: [[30.5,57],[33.5,57],[32,60]] }],
  },
  aigle: {
    id: 'aigle',
    set: 'minimal',
    facets: [
      { points: [[12,12],[36,8],[42,26]], opacity: 1 },
      { points: [[36,8],[58,30],[44,30]], opacity: 0.8 },
      { points: [[44,30],[58,30],[46,42]], opacity: 0.45 },
      { points: [[12,12],[42,26],[44,30],[46,42],[30,58]], opacity: 0.6 },
      { points: [[12,12],[30,58],[8,44]], opacity: 0.3 },
    ],
    eyes: [{ cx: 36, cy: 18, r: 2 }],
    accents: [],
  },
  cerf: {
    id: 'cerf',
    set: 'minimal',
    facets: [
      { points: [[19,26],[8,4],[27,25]], opacity: 0.85 },
      { points: [[18,17],[4,14],[23,21]], opacity: 0.6 },
      { points: [[45,26],[56,4],[37,25]], opacity: 0.6 },
      { points: [[46,17],[60,14],[41,21]], opacity: 0.4 },
      { points: [[10,26],[22,26],[22,36]], opacity: 0.5 },
      { points: [[54,26],[42,26],[42,36]], opacity: 0.3 },
      { points: [[22,26],[42,26],[32,30]], opacity: 1 },
      { points: [[22,26],[32,30],[32,58]], opacity: 0.6 },
      { points: [[42,26],[32,30],[32,58]], opacity: 0.35 },
    ],
    eyes: [],
    accents: [],
  },
};

/** Renvoie les données d'un animal ; lève une Error si l'id est inconnu. */
export function getAnimal(id: string): Animal {
  const animal = animals[id];
  if (!animal) throw new Error(`Animal inconnu : ${id}`);
  return animal;
}
