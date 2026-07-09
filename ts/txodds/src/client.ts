import axios, { AxiosInstance } from "axios";

/**
 * TxODDS API Client for TxLINE
 * 
 * Provides access to StablePrice consensus odds data for seeding
 * QuadraticMarket prediction markets.
 * 
 * Authentication:
 * - Free tier: Use guest session (no API key required)
 * - Paid tier: Use API token via /api/token/activate
 * 
 * @example
 * ```typescript
 * const client = new TxODDSClient();
 * await client.startGuestSession();
 * 
 * const fixtures = await client.getFixtures();
 * const odds = await client.getOddsSnapshot(fixtureId);
 * ```
 */
export class TxODDSClient {
  private httpClient: AxiosInstance;
  private jwt: string | null = null;
  private apiToken: string | null = null;

  constructor(baseUrl = "https://txline.txodds.com") {
    this.httpClient = axios.create({
      baseURL: baseUrl,
      timeout: 10000,
      headers: {
        "Content-Type": "application/json",
      },
    });
  }

  /**
   * Start a guest session for free tier access.
   * Guest sessions are rate-limited and should not be used for production.
   */
  async startGuestSession(): Promise<string> {
    const response = await this.httpClient.post<{ jwt: string }>("/auth/guest/start");
    this.jwt = response.data.jwt;
    return this.jwt;
  }

  /**
   * Activate an API token for paid tier access.
   * @param apiToken - Your TxLINE API token
   */
  async activateApiToken(apiToken: string): Promise<void> {
    this.apiToken = apiToken;
    const response = await this.httpClient.post<{ jwt: string }>(
      "/api/token/activate",
      {},
      {
        headers: {
          "X-Api-Token": apiToken,
        },
      }
    );
    this.jwt = response.data.jwt;
  }

  /**
   * Get authentication headers for API requests
   */
  private getAuthHeaders(): Record<string, string> {
    const headers: Record<string, string> = {};
    if (this.jwt) {
      headers["Authorization"] = `Bearer ${this.jwt}`;
    }
    if (this.apiToken) {
      headers["X-Api-Token"] = this.apiToken;
    }
    return headers;
  }

  /**
   * Fetch all fixtures (upcoming and current matches)
   */
  async getFixtures(): Promise<Fixture[]> {
    const response = await this.httpClient.get<Fixture[]>("/api/fixtures/snapshot", {
      headers: this.getAuthHeaders(),
    });
    return response.data;
  }

  /**
   * Fetch odds snapshot for a specific fixture
   * @param fixtureId - The fixture ID to fetch odds for
   */
  async getOddsSnapshot(fixtureId: number): Promise<OddsEntry[]> {
    const response = await this.httpClient.get<OddsEntry[]>(
      `/api/odds/snapshot/${fixtureId}`,
      {
        headers: this.getAuthHeaders(),
      }
    );
    return response.data;
  }

  /**
   * Fetch scores snapshot for a specific fixture
   * @param fixtureId - The fixture ID to fetch scores for
   */
  async getScoresSnapshot(fixtureId: number): Promise<ScoreEntry[]> {
    const response = await this.httpClient.get<ScoreEntry[]>(
      `/api/scores/snapshot/${fixtureId}`,
      {
        headers: this.getAuthHeaders(),
      }
    );
    return response.data;
  }

  /**
   * Get the current JWT token
   */
  getJwt(): string | null {
    return this.jwt;
  }

  /**
   * Check if the client is authenticated
   */
  isAuthenticated(): boolean {
    return this.jwt !== null;
  }
}

/**
 * Fixture data from TxODDS
 */
export interface Fixture {
  id: number;
  /** Fixture start time as Unix timestamp */
  startTime: number;
  /** Home team name */
  participant1: string;
  /** Away team name */
  participant2: string;
  /** Whether participant1 is home (1) or away (0) */
  participant1IsHome: number;
  /** League/tournament name */
  league: string;
  /** Sport type (e.g., "football") */
  sport: string;
  /** Current status ("SCHEDULED", "LIVE", "FINISHED", etc.) */
  status: string;
}

/**
 * Odds entry from TxODDS StablePrice
 */
export interface OddsEntry {
  /** Fixture ID */
  fixtureId: number;
  /** Bookmaker/source name */
  bookmaker: string;
  /** Market type (e.g., "1X2", "OVER_UNDER") */
  marketType: string;
  /** Selection name (e.g., "Home", "Draw", "Away") */
  selection: string;
  /** Marginated odds (includes bookmaker vig) */
  oddsMarginated: number;
  /** De-marginated odds (true implied probability, vig removed) */
  oddsDemarginated: number;
  /** Odds timestamp as Unix timestamp */
  timestamp: number;
}

/**
 * Score entry from TxODDS
 */
export interface ScoreEntry {
  /** Fixture ID */
  fixtureId: number;
  /** Score type (e.g., "FULL_TIME", "FIRST_HALF") */
  scoreType: string;
  /** Home team score */
  homeScore: number;
  /** Away team score */
  awayScore: number;
  /** Score timestamp as Unix timestamp */
  timestamp: number;
}
